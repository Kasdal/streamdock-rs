param(
    [string]$Version = "0.3.2"
)

$ErrorActionPreference = "Stop"

# The GNU toolchain needs its linker and runtime on PATH.
$mingw = "C:\Users\plesm\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
$selfContained = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained"
$env:Path = "$env:USERPROFILE\.cargo\bin;$mingw;$selfContained;$env:Path"

Write-Host "Building Stream Dock $Version"
# Cargo writes its progress to stderr. Windows PowerShell turns that into a
# terminating error while $ErrorActionPreference is "Stop", so lower it here
# and check the exit code instead.
$ErrorActionPreference = "Continue"
& cargo build --release 2>&1 | ForEach-Object { Write-Host $_ }
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed"
}
$ErrorActionPreference = "Stop"

$dist = "dist\StreamDock-$Version"
if (Test-Path $dist) {
    Remove-Item $dist -Recurse -Force
}
New-Item -ItemType Directory -Force $dist | Out-Null

Copy-Item "target\release\streamdock.exe", "target\release\streamdockd.exe" $dist
$desktop = "crates\desktop\target\x86_64-pc-windows-msvc\release\streamdock-desktop.exe"
if (Test-Path $desktop) {
    Copy-Item $desktop $dist
} else {
    Write-Warning "streamdock-desktop.exe is missing. Build it with build-desktop.ps1."
}
Copy-Item "README.md" $dist
Copy-Item "PRIVACY.md" $dist
Copy-Item "crates\render\assets\Roboto-OFL.txt" (Join-Path $dist "LICENSE-Roboto.txt")
Copy-Item "packaging\install.ps1", "packaging\uninstall.ps1", "packaging\sign.ps1" $dist

$zip = "dist\StreamDock-$Version.zip"
if (Test-Path $zip) {
    Remove-Item $zip -Force
}
Compress-Archive -Path "$dist\*" -DestinationPath $zip

Write-Host ""
Write-Host "Package:"
Get-ChildItem $dist | Select-Object Name, Length | Format-Table
Write-Host "Zip: $zip"
