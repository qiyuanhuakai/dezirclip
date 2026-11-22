Unicode true
RequestExecutionLevel user
Name "DezirClip MyGo"
OutFile "${OUTPUT}"
!ifndef INSTALL_FOLDER
!define INSTALL_FOLDER "DezirClip-MyGo"
!endif
InstallDir "$LOCALAPPDATA\Programs\${INSTALL_FOLDER}"
!ifdef MYGO_FAST_COMPRESSION
SetCompressor zlib
!else
SetCompressor /SOLID lzma
!endif
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"
!define KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\DezirClip-MyGo"
!define OWNER "DezirClip-MyGo-installation-v1"
!define MUI_ABORTWARNING
!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"
LangString UpdateFailed ${LANG_ENGLISH} "DezirClip MyGo could not complete the update. Close DezirClip MyGo and its command-line tools, then retry from the app or install the new version from the release page."
LangString UpdateFailed ${LANG_SIMPCHINESE} "DezirClip MyGo 更新失败。请关闭 DezirClip MyGo 及其命令行工具后重试，也可以从发布页下载并安装新版。"
LangString RelaunchFailed ${LANG_ENGLISH} "The new version of DezirClip MyGo was installed. Open it from the Start menu."
LangString RelaunchFailed ${LANG_SIMPCHINESE} "DezirClip MyGo 新版已安装，请从开始菜单打开。"

!macro Init PREFIX
Function ${PREFIX}.onInit
  SetShellVarContext current
  SetRegView 64
  !if "${PREFIX}" == ""
  StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${INSTALL_FOLDER}"
  !endif
  !ifdef MYGO_ARM64
  ${IfNot} ${IsNativeARM64}
    SetErrorLevel 1
    Abort "This installer requires Windows on ARM64."
  ${EndIf}
  !else
  ${IfNot} ${RunningX64}
    SetErrorLevel 1
    Abort "This installer requires 64-bit Windows."
  ${EndIf}
  !endif
FunctionEnd
!macroend
!insertmacro Init ""
!insertmacro Init "un"

Var UpdateParent

!macro TraceUpdate TEXT
  ReadEnvStr $R8 "DZC_INSTALL_PROBE_TRACE"
  FileOpen $R9 "$R8" a
  FileWrite $R9 "${TEXT}$\r$\n"
  FileClose $R9
!macroend

!macro UpdateFailure CODE REASON
  !insertmacro TraceUpdate "Failure ${CODE}: ${REASON}"
  ${If} $UpdateParent != ""
    MessageBox MB_OK|MB_ICONSTOP "$(UpdateFailed)$\r$\n$\r$\n${REASON}"
  ${EndIf}
  SetErrorLevel ${CODE}
  Abort "${REASON}"
!macroend

!macro Unlocked FILE
  ${If} ${FileExists} "$INSTDIR\${FILE}"
  System::Call 'kernel32::CreateFileW(w "$INSTDIR\${FILE}", i 0xC0000000, i 0, p 0, i 3, i 0, p 0) p.r0'
  ${If} $0 == -1
    !insertmacro TraceUpdate "Locked file: ${FILE}"
    !insertmacro UpdateFailure 2 "Close DezirClip MyGo, its background Service and CLI before continuing."
  ${EndIf}
  System::Call 'kernel32::CloseHandle(p r0)'
  ${EndIf}
!macroend

Section "Install"
  ${GetParameters} $6
  ClearErrors
  ${GetOptions} $6 "/UPDATEPARENT=" $UpdateParent
  ${IfNot} ${Errors}
    !insertmacro TraceUpdate "Waiting for $UpdateParent"
    System::Call 'kernel32::OpenProcess(i 0x00100000, i 0, i $UpdateParent) p.r7'
    ${If} $7 P<> 0
      System::Call 'kernel32::WaitForSingleObject(p r7, i 60000) i.r6'
      System::Call 'kernel32::CloseHandle(p r7)'
      !insertmacro TraceUpdate "Parent wait result $6"
      ${If} $6 != 0
        !insertmacro UpdateFailure 8 "Close DezirClip MyGo before continuing the update."
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ReadRegStr $1 HKCU "${KEY}" "InstallLocation"
  ${If} $UpdateParent != ""
    ${If} $1 == ""
      !insertmacro UpdateFailure 3 "The registered MyGo installation is required for an update."
    ${EndIf}
    ReadINIStr $4 "$1\mygo-installation.ini" "Installation" "Owner"
    ReadINIStr $5 "$1\mygo-installation.ini" "Installation" "Directory"
    ${If} $4 != "${OWNER}"
    ${OrIf} $5 != $1
      !insertmacro UpdateFailure 3 "The registered MyGo installation ownership record is invalid."
    ${EndIf}
    StrCpy $INSTDIR $1
  ${EndIf}
  ${If} $1 != ""
  ${AndIf} $1 != $INSTDIR
    SetErrorLevel 3
    Abort "The registered MyGo installation path differs. Uninstall it first."
  ${EndIf}
  IfFileExists "$INSTDIR\*.*" 0 empty
  ${If} $1 != $INSTDIR
    ReadINIStr $4 "$INSTDIR\mygo-installation.ini" "Installation" "Owner"
    ReadINIStr $5 "$INSTDIR\mygo-installation.ini" "Installation" "Directory"
    ${If} $4 != "${OWNER}"
    ${OrIf} $5 != $INSTDIR
      SetErrorLevel 3
      Abort "The MyGo directory exists without a matching installation ownership record."
    ${EndIf}
  ${EndIf}
empty:
  !insertmacro Unlocked "dzc-desktop.exe"
  !insertmacro Unlocked "dzc-service.exe"
  !insertmacro Unlocked "dzc.exe"
  ClearErrors
  CreateDirectory "$INSTDIR"
  ${IfNot} ${FileExists} "$INSTDIR\mygo-installation.ini"
    FileOpen $0 "$INSTDIR\mygo-installation.ini" w
    FileWriteWord $0 65279
    FileClose $0
  ${EndIf}
  WriteINIStr "$INSTDIR\mygo-installation.ini" "Installation" "Owner" "${OWNER}"
  WriteINIStr "$INSTDIR\mygo-installation.ini" "Installation" "Directory" "$INSTDIR"
  ${If} ${Errors}
    !insertmacro UpdateFailure 6 "Could not record ownership of the MyGo installation directory."
  ${EndIf}
  FlushINI "$INSTDIR\mygo-installation.ini"
  ClearErrors
  ReadINIStr $4 "$INSTDIR\mygo-installation.ini" "Installation" "Owner"
  ReadINIStr $5 "$INSTDIR\mygo-installation.ini" "Installation" "Directory"
  ${If} ${Errors}
  ${OrIf} $4 != "${OWNER}"
  ${OrIf} $5 != $INSTDIR
    !insertmacro UpdateFailure 7 "Could not verify the MyGo installation ownership record."
  ${EndIf}
  SetOverwrite on
  ClearErrors
  !include "${INSTALL_FILES}"
  ${If} ${Errors}
    !insertmacro UpdateFailure 5 "Could not write the complete MyGo application."
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
  ${If} $UpdateParent != ""
    ClearErrors
    Exec '"$INSTDIR\dzc-desktop.exe" --show'
    ${If} ${Errors}
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(RelaunchFailed)"
      SetErrorLevel 9
      Abort "$(RelaunchFailed)"
    ${EndIf}
  ${EndIf}
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
  ; Keep the directory receipt for reinstall and interrupted-install recovery.
  RMDir "$INSTDIR"
  SetErrorLevel 0
SectionEnd
