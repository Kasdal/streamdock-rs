<#
.SYNOPSIS
    Write one version number into every file that carries it.

.DESCRIPTION
    The version appears in the workspace manifest, the desktop manifest, the
    Tauri config, the build scripts, the NSIS script, the README and the local
    project notes. This script keeps them equal, so a release cannot ship two
    numbers.

    The local project notes are not in the repository, so that file is
    optional: the check skips it when it is absent.

    Run it with -Check to compare the files and write nothing. The CI
    workflow runs that check on every push.

.PARAMETER Version
    The version to write: three numbers, for example 0.4.0.

.PARAMETER Check
    Compare the files with the version in Cargo.toml and fail on a mismatch.

.EXAMPLE
    .\scripts\Set-Version.ps1 0.4.0

.EXAMPLE
    .\scripts\Set-Version.ps1 -Check
#>
param(
    [Parameter(Position = 0)]
    [string]$Version,

    [switch]$Check
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

# Every file that carries the version, with the patterns that find it.
# Group 1 of each pattern is the version itself.
$targets = @(
    @{
        Path     = "Cargo.toml"
        Patterns = @('(?m)^version = "(\d+\.\d+\.\d+)"')
    },
    @{
        Path     = "crates\desktop\Cargo.toml"
        Patterns = @('(?m)^version = "(\d+\.\d+\.\d+)"')
    },
    @{
        Path     = "crates\desktop\tauri.conf.json"
        Patterns = @('"version": "(\d+\.\d+\.\d+)"')
    },
    @{
        Path     = "build-release.ps1"
        Patterns = @('\[string\]\$Version = "(\d+\.\d+\.\d+)"')
    },
    @{
        Path     = "build-nsis.ps1"
        Patterns = @('\[string\]\$Version = "(\d+\.\d+\.\d+)"')
    },
    @{
        Path     = "packaging\install.ps1"
        Patterns = @('\[string\]\$Version = "(\d+\.\d+\.\d+)"')
    },
    @{
        Path     = "packaging\sign.ps1"
        Patterns = @('\[string\]\$Folder = "dist\\StreamDock-(\d+\.\d+\.\d+)"')
    },
    @{
        Path     = "packaging\streamdock.nsi"
        Patterns = @('(?<![\w.])(\d+\.\d+\.\d+)(?![\w.])')
    },
    @{
        Path     = "README.md"
        Patterns = @('StreamDock-(\d+\.\d+\.\d+)')
    },
    @{
        Path     = "ProjectData.md"
        Optional = $true
        Patterns = @(
            'StreamDock-(\d+\.\d+\.\d+)',
            '"version": "(\d+\.\d+\.\d+)"'
        )
    }
)

function Get-ManifestVersion {
    $text = [System.IO.File]::ReadAllText((Join-Path $root "Cargo.toml"))
    $match = [regex]::Match($text, '(?m)^version = "(\d+\.\d+\.\d+)"')
    if (-not $match.Success) {
        throw "Cargo.toml has no workspace version"
    }
    return $match.Groups[1].Value
}

$current = Get-ManifestVersion

if ($Check) {
    $expected = $current
    if ($Version) {
        $expected = $Version
    }
} else {
    if (-not $Version) {
        throw "Give a version, for example .\scripts\Set-Version.ps1 0.4.0"
    }
    if ($Version -notmatch '^\d+\.\d+\.\d+$') {
        throw "$Version is not three numbers, for example 0.4.0"
    }
    $expected = $Version
}

$changed = 0
foreach ($target in $targets) {
    $path = Join-Path $root $target.Path
    if (-not (Test-Path $path)) {
        if ($target.Optional) {
            continue
        }
        throw "$($target.Path) is missing"
    }

    $text = [System.IO.File]::ReadAllText($path)
    $updated = $text

    foreach ($pattern in $target.Patterns) {
        $regex = [regex]::new($pattern)
        $found = $regex.Matches($text)
        if ($found.Count -eq 0) {
            throw "$($target.Path): the pattern $pattern found nothing"
        }
        foreach ($match in $found) {
            $value = $match.Groups[1].Value
            if ($value -ne $current -and $value -ne $expected) {
                throw "$($target.Path): found $value, expected $current or $expected"
            }
        }
        if (-not $Check) {
            $updated = $regex.Replace($updated, {
                    param($match)
                    $match.Value.Replace($match.Groups[1].Value, $expected)
                })
        }
    }

    if (-not $Check -and $updated -ne $text) {
        # Write UTF-8 without a byte order mark and keep the line endings.
        [System.IO.File]::WriteAllText($path, $updated, [System.Text.UTF8Encoding]::new($false))
        Write-Host "wrote $($target.Path)"
        $changed++
    }
}

if ($Check) {
    Write-Host "version $expected is in every file"
} else {
    Write-Host "version $expected written to $changed files"
}
