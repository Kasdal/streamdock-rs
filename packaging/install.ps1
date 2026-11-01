param(
    [string]$Target = "$env:LOCALAPPDATA\StreamDock\bin",
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
foreach ($name in @("streamdock.exe", "streamdockd.exe", "README.md", "LICENSE-Roboto.txt")) {
    $from = Join-Path $source $name
    if (Test-Path $from) {
        Copy-Item $from $Target -Force
    }
}

Get-Process streamdockd -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300

if (-not $NoAutostart) {
    & (Join-Path $Target "streamdock.exe") autostart install
}

if (-not $NoShortcut) {
    $startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut((Join-Path $startMenu "Stream Dock.lnk"))
    $link.TargetPath = Join-Path $Target "streamdockd.exe"
    $link.WorkingDirectory = $Target
    $link.IconLocation = (Join-Path $Target "streamdockd.exe") + ",0"
    $link.Description = "Stream Dock panel daemon"
    $link.Save()
}

Write-Host ""
Write-Host "Installed. Start the daemon from the Start menu, or run:"
Write-Host "  $(Join-Path $Target 'streamdockd.exe')"
