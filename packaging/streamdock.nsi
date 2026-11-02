; Stream Dock NSIS installer.
;
; Build it with build-nsis.ps1, or by hand:
;   makensis /DVERSION=0.3.1 /DSRCDIR=C:\path\dist\StreamDock-0.3.1 ^
;            /DOUTFILE=C:\path\dist\StreamDock-0.3.1-setup.exe streamdock.nsi
;
; The installer is per user: no administrator rights, and the files go to
; %LOCALAPPDATA%. Uninstall keeps the profiles.

Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.3.1"
!endif
!ifndef SRCDIR
  !define SRCDIR "..\dist\StreamDock-${VERSION}"
!endif
!ifndef OUTFILE
  !define OUTFILE "..\dist\StreamDock-${VERSION}-setup.exe"
!endif

!define APPNAME "Stream Dock"
!define PUBLISHER "Stream Dock"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\StreamDock"

Name "${APPNAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\StreamDock\bin"
InstallDirRegKey HKCU "Software\StreamDock" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${APPNAME}"

Icon "..\crates\daemon\assets\streamdock.ico"
UninstallIcon "..\crates\daemon\assets\streamdock.ico"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "FileDescription" "Stream Dock installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${SRCDIR}\streamdock.exe"
  File "${SRCDIR}\streamdockd.exe"
  !if /FileExists "${SRCDIR}\streamdock-desktop.exe"
    File "${SRCDIR}\streamdock-desktop.exe"
  !endif
  File "${SRCDIR}\README.md"
  File "${SRCDIR}\LICENSE-Roboto.txt"
  File "${SRCDIR}\PRIVACY.md"

  ; Stop a running daemon so the files can be replaced.
  nsExec::Exec 'taskkill /IM streamdockd.exe /F'
  Pop $0
  nsExec::Exec 'taskkill /IM streamdock-desktop.exe /F'
  Pop $0

  ; Autostart and shortcuts.
  nsExec::Exec '"$INSTDIR\streamdock.exe" autostart install'
  Pop $0

  CreateDirectory "$SMPROGRAMS\Stream Dock"
  !if /FileExists "${SRCDIR}\streamdock-desktop.exe"
    CreateShortCut "$SMPROGRAMS\Stream Dock\Stream Dock.lnk" "$INSTDIR\streamdock-desktop.exe" "" "$INSTDIR\streamdock-desktop.exe" 0
  !else
    CreateShortCut "$SMPROGRAMS\Stream Dock\Stream Dock.lnk" "$INSTDIR\streamdockd.exe" "" "$INSTDIR\streamdockd.exe" 0
  !endif
  CreateShortCut "$SMPROGRAMS\Stream Dock\Stream Dock Editor.lnk" "$INSTDIR\streamdock.exe" "ui"
  CreateShortCut "$SMPROGRAMS\Stream Dock\Uninstall Stream Dock.lnk" "$INSTDIR\uninstall.exe"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "Software\StreamDock" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\streamdockd.exe"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM streamdockd.exe /F'
  Pop $0
  nsExec::Exec 'taskkill /IM streamdock-desktop.exe /F'
  Pop $0
  nsExec::Exec '"$INSTDIR\streamdock.exe" autostart remove'
  Pop $0

  Delete "$SMPROGRAMS\Stream Dock\Stream Dock.lnk"
  Delete "$SMPROGRAMS\Stream Dock\Stream Dock Editor.lnk"
  Delete "$SMPROGRAMS\Stream Dock\Uninstall Stream Dock.lnk"
  RMDir "$SMPROGRAMS\Stream Dock"

  Delete "$INSTDIR\streamdock.exe"
  Delete "$INSTDIR\streamdockd.exe"
  Delete "$INSTDIR\streamdock-desktop.exe"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE-Roboto.txt"
  Delete "$INSTDIR\PRIVACY.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "Software\StreamDock"
SectionEnd
