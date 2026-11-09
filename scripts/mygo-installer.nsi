Unicode true
RequestExecutionLevel user
Name "DezirClip MyGo"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\DezirClip-MyGo"
SetCompressor /SOLID lzma
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!define KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\DezirClip-MyGo"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

!macro Init PREFIX
Function ${PREFIX}.onInit
  SetShellVarContext current
  SetRegView 64
  StrCpy $INSTDIR "$LOCALAPPDATA\Programs\DezirClip-MyGo"
  ${IfNot} ${RunningX64}
    SetErrorLevel 1
    Abort "This installer requires 64-bit Windows."
  ${EndIf}
FunctionEnd
!macroend
!insertmacro Init ""
!insertmacro Init "un"

!macro Unlocked FILE
  ${If} ${FileExists} "$INSTDIR\${FILE}"
  System::Call 'kernel32::CreateFileW(w "$INSTDIR\${FILE}", i 0xC0000000, i 0, p 0, i 3, i 0, p 0) p.r0'
  ${If} $0 == -1
    SetErrorLevel 2
    Abort "Close DezirClip MyGo, its background Service and CLI before continuing."
  ${EndIf}
  System::Call 'kernel32::CloseHandle(p r0)'
  ${EndIf}
!macroend

Section "Install"
  SetRegView 32
  ReadRegStr $2 HKLM "Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ReadRegStr $3 HKCU "Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  SetRegView 64
  ${If} $2 == ""
  ${OrIf} $2 == "0.0.0.0"
    ${If} $3 == ""
    ${OrIf} $3 == "0.0.0.0"
      SetErrorLevel 4
      Abort "Install Microsoft Edge WebView2 Runtime first: https://developer.microsoft.com/microsoft-edge/webview2/"
    ${EndIf}
  ${EndIf}
  ReadRegStr $1 HKCU "${KEY}" "InstallLocation"
  ${If} $1 != ""
  ${AndIf} $1 != $INSTDIR
    SetErrorLevel 3
    Abort "The registered MyGo installation path differs. Uninstall it first."
  ${EndIf}
  IfFileExists "$INSTDIR\*.*" 0 empty
  ${If} $1 != $INSTDIR
    SetErrorLevel 3
    Abort "The MyGo installation directory already exists without an installation receipt."
  ${EndIf}
empty:
  !insertmacro Unlocked "dzc-desktop.exe"
  !insertmacro Unlocked "dzc-service.exe"
  !insertmacro Unlocked "dzc.exe"
  SetOverwrite on
  ClearErrors
  !include "${INSTALL_FILES}"
  ${If} ${Errors}
    SetErrorLevel 5
    Abort "Could not write the complete MyGo application."
  ${EndIf}
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateDirectory "$SMPROGRAMS\DezirClip MyGo"
  CreateShortcut "$SMPROGRAMS\DezirClip MyGo\DezirClip MyGo.lnk" "$INSTDIR\dzc-desktop.exe"
  CreateShortcut "$DESKTOP\DezirClip MyGo.lnk" "$INSTDIR\dzc-desktop.exe"
  WriteRegStr HKCU "${KEY}" "DisplayName" "DezirClip MyGo"
  WriteRegStr HKCU "${KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${KEY}" "Publisher" "DezirClip"
  WriteRegStr HKCU "${KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${KEY}" "DisplayIcon" "$INSTDIR\dzc-desktop.exe"
  WriteRegStr HKCU "${KEY}" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  WriteRegStr HKCU "${KEY}" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
  WriteRegDWORD HKCU "${KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${KEY}" "NoRepair" 1
  SetErrorLevel 0
SectionEnd

Section "Uninstall"
  ReadRegStr $1 HKCU "${KEY}" "InstallLocation"
  ${If} $1 != $INSTDIR
    SetErrorLevel 3
    Abort "The MyGo installation receipt is missing or belongs to another directory."
  ${EndIf}
  !insertmacro Unlocked "dzc-desktop.exe"
  !insertmacro Unlocked "dzc-service.exe"
  !insertmacro Unlocked "dzc.exe"
  ClearErrors
  !include "${REMOVE_FILES}"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$SMPROGRAMS\DezirClip MyGo\DezirClip MyGo.lnk"
  RMDir "$SMPROGRAMS\DezirClip MyGo"
  Delete "$DESKTOP\DezirClip MyGo.lnk"
  DeleteRegKey HKCU "${KEY}"
  RMDir "$INSTDIR"
  SetErrorLevel 0
SectionEnd
