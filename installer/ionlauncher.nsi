; IonLauncher installer.
;
; A multi-page wizard: welcome, licence, install location, the desktop
; shortcut choice, install, then a finish page that can start the app.
; The shortcut page is a custom page because NSIS's built-in one only
; offers start-menu and quick-launch options.

Unicode true

; Build a 64-bit installer. IonLauncher ships as an x86_64 binary, so there is
; nothing for a 32-bit stub to install, and the 64-bit stub makes
; $PROGRAMFILES64 and the Program Files redirects behave natively.
Target amd64-unicode

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "nsDialogs.nsh"

!define APP_NAME        "IonLauncher"
!define APP_EXE         "ionlauncher.exe"
!define APP_PUBLISHER   "IonLauncher"
!define APP_URL         "https://github.com/ciao22king/IonLauncher"
!define UNINST_KEY      "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

; The version lives in Cargo.toml, and the build passes it with
; -DAPP_VERSION so the installer cannot drift from the binary it wraps. The
; fallback keeps a bare `makensis ionlauncher.nsi` working.
!ifndef APP_VERSION
  !define APP_VERSION "0.1.0"
!endif

; VIProductVersion wants four numeric fields; the package version has three.
!define APP_VERSION_4 "${APP_VERSION}.0"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "..\dist\IonLauncher-${APP_VERSION}-Setup.exe"
InstallDir "$PROGRAMFILES64\${APP_NAME}"
InstallDirRegKey HKLM "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
ShowInstDetails show

VIProductVersion "${APP_VERSION_4}"
VIAddVersionKey "ProductName"     "${APP_NAME}"
VIAddVersionKey "FileDescription" "${APP_NAME} Setup"
VIAddVersionKey "FileVersion"     "${APP_VERSION}"
VIAddVersionKey "ProductVersion"  "${APP_VERSION}"
VIAddVersionKey "CompanyName"     "${APP_PUBLISHER}"
VIAddVersionKey "LegalCopyright"  "GPL-2.0-or-later"

!define MUI_ABORTWARNING
!define MUI_ICON   "..\assets\ionlauncher.ico"
!define MUI_UNICON "..\assets\ionlauncher.ico"

!define MUI_WELCOMEPAGE_TITLE "Welcome to the ${APP_NAME} setup wizard"
!define MUI_WELCOMEPAGE_TEXT "This wizard installs ${APP_NAME}, a single fast \
launcher for your Steam, Epic Games and GOG libraries.$\r$\n$\r$\n\
${APP_NAME} reads each store's own files on your PC. It has no account, sends \
nothing anywhere, and does not touch your game installations.$\r$\n$\r$\n\
Click Next to continue."
!insertmacro MUI_PAGE_WELCOME

!define MUI_LICENSEPAGE_TEXT_TOP "Please read the licence before installing ${APP_NAME}."
!define MUI_LICENSEPAGE_BUTTON "I accept"
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"

!insertmacro MUI_PAGE_DIRECTORY
Page custom ShortcutPageCreate ShortcutPageLeave

!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Run ${APP_NAME}"
!define MUI_FINISHPAGE_LINK "Project page on GitHub"
!define MUI_FINISHPAGE_LINK_LOCATION "${APP_URL}"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Which shortcuts to create. 1 = create, 0 = skip.
Var MakeDesktopShortcut
Var MakeStartMenuShortcut

; Control handles. These must live in variables rather than registers: MUI
; reuses $0-$9 while moving between pages, so a handle kept in a register is
; clobbered before the leave function reads it and the choice is lost.
Var DesktopCheckbox
Var StartMenuCheckbox
Var ShortcutIntro

Function .onInit
  StrCpy $MakeDesktopShortcut 1
  StrCpy $MakeStartMenuShortcut 1

  ; $DESKTOP and $SMPROGRAMS resolve to the *elevating* account's folders.
  ; The installer runs elevated, so without this the shortcut would land on
  ; the administrator's desktop instead of the one the user actually sees.
  SetShellVarContext all

  ; Pre-select the previous install location when upgrading.
  ReadRegStr $0 HKLM "${UNINST_KEY}" "InstallLocation"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
FunctionEnd

; Custom wizard page holding the two shortcut checkboxes.
Function ShortcutPageCreate
  !insertmacro MUI_HEADER_TEXT "Shortcuts" "Choose where ${APP_NAME} should appear."

  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 28u \
    "Choose the shortcuts to create. You can change these later by running this setup again."
  Pop $ShortcutIntro

  ${NSD_CreateCheckbox} 0 34u 100% 12u "Create a shortcut on the Desktop"
  Pop $DesktopCheckbox
  ${If} $MakeDesktopShortcut == 1
    ${NSD_SetState} $DesktopCheckbox ${BST_CHECKED}
  ${EndIf}

  ${NSD_CreateCheckbox} 0 50u 100% 12u "Create a shortcut in the Start menu"
  Pop $StartMenuCheckbox
  ${If} $MakeStartMenuShortcut == 1
    ${NSD_SetState} $StartMenuCheckbox ${BST_CHECKED}
  ${EndIf}

  nsDialogs::Show
FunctionEnd

Function ShortcutPageLeave
  ${NSD_GetState} $DesktopCheckbox $MakeDesktopShortcut
  ${NSD_GetState} $StartMenuCheckbox $MakeStartMenuShortcut
FunctionEnd

Section "IonLauncher" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  SetOverwrite on

  File "..\dist\${APP_EXE}"
  File "..\LICENSE"
  File "..\README.md"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName"     "${APP_NAME}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion"  "${APP_VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher"       "${APP_PUBLISHER}"
  WriteRegStr HKLM "${UNINST_KEY}" "URLInfoAbout"    "${APP_URL}"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon"     "$INSTDIR\${APP_EXE}"
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

; Shortcuts live in their own section so the checkbox state alone decides
; whether they exist, and so uninstall can remove them by path.
Section "Shortcuts" SecShortcuts
  ${If} $MakeDesktopShortcut == ${BST_CHECKED}
    CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" \
      "$INSTDIR\${APP_EXE}" 0 SW_SHOWNORMAL "" "Play your Steam, Epic and GOG games"
  ${EndIf}

  ${If} $MakeStartMenuShortcut == ${BST_CHECKED}
    CreateDirectory "$SMPROGRAMS\${APP_NAME}"
    CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" \
      "$INSTDIR\${APP_EXE}" 0 SW_SHOWNORMAL "" "Play your Steam, Epic and GOG games"
    CreateShortCut "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk" "$INSTDIR\uninstall.exe"
  ${EndIf}
SectionEnd

Section "Uninstall"
  ; Must match the context used at install time, or the uninstaller looks
  ; for the shortcut in the wrong profile and leaves it behind.
  SetShellVarContext all

  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"

  DeleteRegKey HKLM "${UNINST_KEY}"

  ; The user's own settings and cover cache are deliberately left in place.
SectionEnd
