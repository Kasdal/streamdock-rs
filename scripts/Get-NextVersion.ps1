<#
.SYNOPSIS
    Work out the next version from the commit messages.

.DESCRIPTION
    The script reads the commits after the last vX.Y.Z tag and maps them to a
    semantic version bump:

    | Commit | Bump |
    | --- | --- |
    | `feat:` | minor |
    | `fix:`, `perf:`, `revert:` | patch |
    | any type with `!`, or a `BREAKING CHANGE:` footer | major |
    | `docs:`, `chore:`, `ci:`, `test:`, `style:`, `build:` | none |
    | any other message | patch |

    A commit message may override the result with `[release:major]`,
    `[release:minor]` or `[release:patch]`. The newest commit in the range may
    stop a release with `[skip release]`.

    The next version goes to standard output and nothing else, so the caller
    can capture it. The reason goes to the log. When no commit asks for a
    release the script prints a warning and writes nothing.

.PARAMETER From
    Start the range after this git ref. The last tag wins when one exists.
    Use this for a first release, when no tag exists yet.

.PARAMETER Bump
    Force a bump and ignore the commit messages. The default is auto.

.EXAMPLE
    .\scripts\Get-NextVersion.ps1

.EXAMPLE
    $next = .\scripts\Get-NextVersion.ps1 -Bump minor
#>
param(
    [string]$From = "",

    [ValidateSet("auto", "patch", "minor", "major")]
    [string]$Bump = "auto"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

function Get-ManifestVersion {
    $text = [System.IO.File]::ReadAllText((Join-Path $root "Cargo.toml"))
    $match = [regex]::Match($text, '(?m)^version = "(\d+\.\d+\.\d+)"')
    if (-not $match.Success) {
        throw "Cargo.toml has no workspace version"
    }
    return $match.Groups[1].Value
}

Push-Location $root
try {
    $current = Get-ManifestVersion

    # Prefer the last release tag. A push range is the fallback for the first
    # release, when the repository has no tag yet. `git tag` stays quiet when
    # the list is empty, unlike `git describe`.
    $range = "HEAD"
    $single = $false
    $tag = & git tag --list "v[0-9]*" --sort=-v:refname | Select-Object -First 1
    if ($tag) {
        $range = "$tag..HEAD"
        Write-Host "commits since the tag $tag"
    } elseif ($From -and $From -notmatch '^0+$') {
        $range = "$From..HEAD"
        Write-Host "commits since $From"
    } else {
        $single = $true
        Write-Host "no tag and no start point, so only the last commit counts"
    }

    # `git log HEAD` lists the whole history, so a single commit needs -1.
    $logArgs = @()
    if ($single) {
        $logArgs += "-1"
    }
    $logArgs += $range

    $subjects = & git log --format=%s @logArgs
    if ($LASTEXITCODE -ne 0) {
        throw "git log failed for $range"
    }
    $bodies = & git log --format=%b @logArgs
    $text = ($subjects + $bodies) -join "`n"
    # The skip marker belongs to one push, so only the newest commit counts.
    $newest = & git log -1 --format=%B $range

    # A subject that names no type still ships a fix, so a plain message
    # counts as a patch. Only the housekeeping types make no release.
    $isMajor = $false
    $isMinor = $false
    $isPatch = $false
    foreach ($subject in $subjects) {
        if ($subject -match '^[a-z]+(\([^)]*\))?!:') {
            $isMajor = $true
        } elseif ($subject -match '^feat(\([^)]*\))?:') {
            $isMinor = $true
        } elseif ($subject -match '^(fix|perf|revert)(\([^)]*\))?:') {
            $isPatch = $true
        } elseif ($subject -match '^(docs|chore|ci|test|style|build)(\([^)]*\))?:') {
            # Housekeeping alone does not change the program.
        } else {
            $isPatch = $true
        }
    }
    if ($bodies -match '(?m)^BREAKING[ -]CHANGE:') {
        $isMajor = $true
    }

    $level = $null
    if ($isMajor) {
        $level = "major"
    } elseif ($isMinor) {
        $level = "minor"
    } elseif ($isPatch) {
        $level = "patch"
    }

    if ($text -match '(?im)\[release:(major|minor|patch)\]') {
        $level = $Matches[1]
    }
    if ($newest -match '(?im)\[skip release\]') {
        $level = $null
    }
    if ($Bump -ne "auto") {
        $level = $Bump
    }

    if (-not $level) {
        Write-Warning "no commit asks for a release"
        return
    }

    $parts = $current.Split('.')
    $major = [int]$parts[0]
    $minor = [int]$parts[1]
    $patch = [int]$parts[2]

    switch ($level) {
        "major" { $major += 1; $minor = 0; $patch = 0 }
        "minor" { $minor += 1; $patch = 0 }
        "patch" { $patch += 1 }
    }

    Write-Host "$current plus a $level bump"
    Write-Output "$major.$minor.$patch"
} finally {
    Pop-Location
}
