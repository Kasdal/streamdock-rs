param(
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path

# The desktop window is a Tauri app, so it needs the MSVC toolchain. The rest
# of the project builds with GNU, so this crate is outside the workspace.
$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $install = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($install) {
            $vcvars = Join-Path $install "VC\Auxiliary\Build\vcvars64.bat"
        }
    }
}
if (-not (Test-Path $vcvars)) {
    throw "vcvars64.bat was not found. Install the MSVC build tools."
}

Write-Host "Loading the MSVC environment from $vcvars"
cmd /c "`"$vcvars`" >nul && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') {
        Set-Item -Path "env:$($matches[1])" -Value $matches[2]
    }
}

$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

# The host toolchain is GNU, so host-side build scripts still need dlltool
# from WinLibs. The target is MSVC and links with link.exe from vcvars.
$mingw = "C:\Users\plesm\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
$selfContained = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained"
if (Test-Path $mingw) {
    $env:Path = "$mingw;$selfContained;$env:Path"
}

Write-Host "Building the desktop window"
# Cargo writes its progress to stderr. Windows PowerShell turns that into a
# terminating error while $ErrorActionPreference is "Stop", so lower it here
# and check the exit code instead.
$ErrorActionPreference = "Continue"
& cargo build --$Configuration --manifest-path (Join-Path $root "crates\desktop\Cargo.toml") --target x86_64-pc-windows-msvc 2>&1 | ForEach-Object { Write-Host $_ }
if ($LASTEXITCODE -ne 0) {
    throw "the desktop build failed"
}

$exe = Join-Path $root "crates\desktop\target\x86_64-pc-windows-msvc\$Configuration\streamdock-desktop.exe"
if (-not (Test-Path $exe)) {
    throw "$exe is missing"
}
Write-Host "wrote $exe"
Get-Item $exe | Select-Object Name, Length
