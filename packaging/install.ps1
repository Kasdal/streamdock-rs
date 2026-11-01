param(
    [string]$Target = "$env:LOCALAPPDATA\StreamDock\bin",
    [string]$Version = "0.2.0",
    [switch]$NoAutostart,
    [switch]$NoShortcut
)

$ErrorActionPreference = "Stop"
$source = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host "Stream Dock installer"
Write-Host "  source: $source"
Write-Host "  target: $Target"
Write-Host ""

New-Item -ItemType Directory -Force $Target | Out-Null
foreach ($name in @(
    "streamdock.exe",
    "streamdockd.exe",
    "streamdock-desktop.exe",
    "README.md",
    "PRIVACY.md",
    "LICENSE-Roboto.txt",
    "uninstall.ps1"
)) {
    $from = Join-Path $source $name
    if (Test-Path $from) {
        Copy-Item $from $Target -Force
    }
}

Get-Process streamdockd, streamdock-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300

if (-not $NoAutostart) {
    & (Join-Path $Target "streamdock.exe") autostart install
}

if (-not $NoShortcut) {
    $startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut((Join-Path $startMenu "Stream Dock.lnk"))
    $app = Join-Path $Target "streamdock-desktop.exe"
    if (-not (Test-Path $app)) {
        $app = Join-Path $Target "streamdockd.exe"
    }
    $link.TargetPath = $app
    $link.WorkingDirectory = $Target
    $link.IconLocation = (Join-Path $Target "streamdockd.exe") + ",0"
    $link.Description = "Stream Dock"
    $link.Save()
}

# Add an entry to "Apps and features" for this user.
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\StreamDock"
New-Item -Path $uninstallKey -Force | Out-Null
Set-ItemProperty -Path $uninstallKey -Name "DisplayName" -Value "Stream Dock"
Set-ItemProperty -Path $uninstallKey -Name "DisplayVersion" -Value $Version
Set-ItemProperty -Path $uninstallKey -Name "Publisher" -Value "Stream Dock"
Set-ItemProperty -Path $uninstallKey -Name "DisplayIcon" -Value (Join-Path $Target "streamdockd.exe")
Set-ItemProperty -Path $uninstallKey -Name "InstallLocation" -Value $Target
Set-ItemProperty -Path $uninstallKey -Name "UninstallString" -Value (
    'powershell -NoProfile -ExecutionPolicy Bypass -File "' +
    (Join-Path $Target "uninstall.ps1") + '"'
)
New-ItemProperty -Path $uninstallKey -Name "NoModify" -Value 1 -PropertyType DWord -Force | Out-Null
New-ItemProperty -Path $uninstallKey -Name "NoRepair" -Value 1 -PropertyType DWord -Force | Out-Null

Write-Host ""
Write-Host "Installed. Start the daemon from the Start menu, or run:"
Write-Host "  $(Join-Path $Target 'streamdockd.exe')"
