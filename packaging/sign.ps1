param(
    [string]$Pfx = $env:STREAMDOCK_CERT,
    [string]$Password = $env:STREAMDOCK_CERT_PASSWORD,
    [string]$Folder = "dist\StreamDock-0.3.2"
)

$ErrorActionPreference = "Stop"

if (-not $Pfx) {
    Write-Error "Set STREAMDOCK_CERT to the path of a .pfx file, and STREAMDOCK_CERT_PASSWORD to its password."
    exit 1
}

$signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
if (-not $signtool) {
    Write-Error "signtool.exe was not found. Install the Windows SDK signing tools, or use the SDK Developer PowerShell."
    exit 1
}

foreach ($name in @("streamdock.exe", "streamdockd.exe")) {
    $path = Join-Path $Folder $name
    if (-not (Test-Path $path)) {
        Write-Error "$path is missing. Run build-release.ps1 first."
    }
    & $signtool.Source sign /f $Pfx /p $Password /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com $path
    if ($LASTEXITCODE -ne 0) {
        Write-Error "signing failed for $path"
    }
    Write-Host "signed $path"
}
