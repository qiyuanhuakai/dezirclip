!include "LogicLib.nsh"

; Every packaged file is required, including during a silent update.
AllowSkipFiles off

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode = 1
    DetailPrint "Keeping Windows settings during update."
  ${Else}
    ; Finish the running desktop process before restoring settings, then wait for
    ; the maintenance process to exit before the standard template deletes it.
    !ifmacrodef RestartManager_RegisterFile
      !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    !else
      !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    !endif
    IfFileExists "$INSTDIR\${MAINBINARYNAME}.exe" 0 dzc_restore_missing
    nsExec::ExecToStack '"$INSTDIR\${MAINBINARYNAME}.exe" --restore-system-settings'
    Pop $0
    Pop $1
    ${If} $0 != 0
      DetailPrint "Windows settings restoration failed; application files have been kept."
      SetErrorLevel 2
      Abort
    ${EndIf}
    Goto dzc_restore_done
  dzc_restore_missing:
    DetailPrint "Windows settings restoration is unavailable; application files have been kept."
    SetErrorLevel 2
    Abort
  dzc_restore_done:
  ${EndIf}
!macroend
