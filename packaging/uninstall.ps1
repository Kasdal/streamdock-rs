param(
    [string]$Target = "$env:LOCALAPPDATA\StreamDock\bin",
    [switch]$KeepProfiles
)

$ErrorActionPreference = "Continue"

$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\StreamDock"
$installed = (Get-ItemProperty -Path $uninstallKey -ErrorAction SilentlyContinue).InstallLocation
if ($installed -and -not $PSBoundParameters.ContainsKey("Target")) {
    $Target = $installed
}

Write-Host "Stream Dock uninstaller"
Write-Host "  target: $Target"

Get-Process streamdockd, streamdock-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300

$cli = Join-Path $Target "streamdock.exe"
if (Test-Path $cli) {
    & $cli autostart remove | Out-Null
}

$link = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Stream Dock.lnk"
Remove-Item $link -ErrorAction SilentlyContinue
Remove-Item $Target -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item $uninstallKey -Recurse -Force -ErrorAction SilentlyContinue

if (-not $KeepProfiles) {
    Remove-Item "$env:LOCALAPPDATA\StreamDock\profiles" -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item "$env:LOCALAPPDATA\StreamDock\logs" -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item "$env:LOCALAPPDATA\StreamDock\runtime.json" -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($KeepProfiles) {
    Write-Host "Removed the program. Profiles, logs and settings were kept."
} else {
    Write-Host "Removed the program, profiles, logs and settings."
}
