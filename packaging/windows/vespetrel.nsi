; NSIS Modern User Interface script for Vespetrel Mail Client
; -------------------------------------------------------------
!include "MUI2.nsh"
!include "FileFunc.nsh"

; General Definitions
!define PRODUCT_NAME "Vespetrel"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_PUBLISHER "Vespetrel Team"
!define PRODUCT_WEB_SITE "https://github.com/SV-stark/Vespetrel"
!define PRODUCT_EXE "vespetrel.exe"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_UNINST_ROOT_KEY "HKLM"

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "..\..\dist\vespetrel-setup-windows-x86_64.exe"
InstallDir "$PROGRAMFILES64\${PRODUCT_NAME}"
InstallDirRegKey HKLM "Software\${PRODUCT_NAME}" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

; UI Settings
!define MUI_ABORTWARNING
!define MUI_ICON "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"

; Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE-MIT"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${PRODUCT_EXE}"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

Section "MainSection" SEC01
    SetOutPath "$INSTDIR"
    SetOverwrite on
    File "..\..\target\release\${PRODUCT_EXE}"
    File "..\..\README.md"
    File "..\..\LICENSE-MIT"

    ; Create Shortcuts
    CreateDirectory "$SMPROGRAMS\${PRODUCT_NAME}"
    CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk" "$INSTDIR\${PRODUCT_EXE}"
    CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall.lnk" "$INSTDIR\uninst.exe"
    CreateShortCut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\${PRODUCT_EXE}"

    ; Register mailto: protocol handler
    WriteRegStr HKCR "mailto" "" "URL:MailTo Protocol"
    WriteRegStr HKCR "mailto" "URL Protocol" ""
    WriteRegStr HKCR "mailto\shell\open\command" "" '"$INSTDIR\${PRODUCT_EXE}" --compose "%1"'

    ; Write Uninstaller registry entries
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "${PRODUCT_NAME}"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninst.exe"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\${PRODUCT_EXE}"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
    WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
    WriteUninstaller "$INSTDIR\uninst.exe"
SectionEnd

Section "Uninstall"
    Delete "$INSTDIR\${PRODUCT_EXE}"
    Delete "$INSTDIR\README.md"
    Delete "$INSTDIR\LICENSE-MIT"
    Delete "$INSTDIR\uninst.exe"

    Delete "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk"
    Delete "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall.lnk"
    Delete "$DESKTOP\${PRODUCT_NAME}.lnk"
    RMDir "$SMPROGRAMS\${PRODUCT_NAME}"
    RMDir "$INSTDIR"

    DeleteRegKey HKCR "mailto"
    DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"
    DeleteRegKey HKLM "Software\${PRODUCT_NAME}"
SectionEnd
