Unicode True
!cd ".."

!include "MUI2.nsh"

Name "Vespetrel"
OutFile "dist\vespetrel-setup-windows-x86_64.exe"
InstallDir "$LOCALAPPDATA\Programs\Vespetrel"
InstallDirRegKey HKCU "Software\Vespetrel" "InstallDir"
RequestExecutionLevel user

!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

!ifndef VERSION
  !define VERSION "0.1.0"
!endif

Section "Install"
    SetOutPath "$INSTDIR"
    File "staging\vespetrel\vespetrel.exe"
    File "staging\vespetrel\README.md"
    File "staging\vespetrel\LICENSE"

    WriteRegStr HKCU "Software\Vespetrel" "InstallDir" "$INSTDIR"

    CreateShortCut "$DESKTOP\Vespetrel.lnk" "$INSTDIR\vespetrel.exe"
    CreateDirectory "$SMPROGRAMS\Vespetrel"
    CreateShortCut "$SMPROGRAMS\Vespetrel\Vespetrel.lnk" "$INSTDIR\vespetrel.exe"
    CreateShortCut "$SMPROGRAMS\Vespetrel\Uninstall Vespetrel.lnk" "$INSTDIR\uninstall.exe"

    WriteUninstaller "$INSTDIR\uninstall.exe"

    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vespetrel" "DisplayName" "Vespetrel Mail Client"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vespetrel" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vespetrel" "DisplayIcon" "$INSTDIR\vespetrel.exe"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vespetrel" "Publisher" "Vespetrel Team"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vespetrel" "DisplayVersion" "${VERSION}"
SectionEnd

Section "Uninstall"
    SetOutPath "$TEMP"

    Delete "$DESKTOP\Vespetrel.lnk"
    Delete "$SMPROGRAMS\Vespetrel\Vespetrel.lnk"
    Delete "$SMPROGRAMS\Vespetrel\Uninstall Vespetrel.lnk"
    RMDir "$SMPROGRAMS\Vespetrel"

    Delete "$INSTDIR\vespetrel.exe"
    Delete "$INSTDIR\README.md"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vespetrel"
    DeleteRegKey HKCU "Software\Vespetrel"
SectionEnd
