param(
    [string]$Version = "0.3.0",
    [string]$MakensisPath = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$source = Join-Path $root "dist\StreamDock-$Version"
if (-not (Test-Path $source)) {
    throw "Run build-release.ps1 first: $source is missing."
}

# Find makensis: explicit path, then PATH, then the usual install folders.
$tool = $null
if ($MakensisPath) {
    if (-not (Test-Path $MakensisPath)) {
        throw "$MakensisPath is missing."
    }
    $tool = Get-Item $MakensisPath
} else {
    $found = Get-Command makensis.exe -ErrorAction SilentlyContinue
    if ($found) {
        $tool = Get-Item -Path $found.Source
    } else {
        foreach ($candidate in @(
            "$env:ProgramFiles\NSIS\makensis.exe",
            "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
        )) {
            if (Test-Path $candidate) {
                $tool = Get-Item $candidate
                break
            }
        }
    }
}

if (-not $tool) {
    Write-Host "makensis.exe was not found."
    Write-Host "Install NSIS with: winget install NSIS.NSIS"
    Write-Host "Or unpack a portable NSIS and pass -MakensisPath PATH."
    Write-Host "The portable package from build-release.ps1 needs no extra tool."
    exit 1
}

$out = Join-Path $root "dist\StreamDock-$Version-setup.exe"
$exe = $tool.FullName
& $exe "/DVERSION=$Version" "/DSRCDIR=$source" "/DOUTFILE=$out" (Join-Path $root "packaging\streamdock.nsi")
if ($LASTEXITCODE -ne 0) {
    throw "makensis failed with exit code $LASTEXITCODE"
}

Write-Host "wrote $out"
Get-Item $out | Select-Object Name, Length
