!include "LogicLib.nsh"

ManifestDPIAware true
SetFont "Segoe UI" 10
SetFont /LANG=2052 "Microsoft YaHei UI" 10

!define MUI_TEXTCOLOR "17283D"
!define MUI_HEADERIMAGE_RIGHT
!define MUI_WELCOMEPAGE_TITLE "$(^Name)"
!define MUI_WELCOMEPAGE_TEXT "$DezirClipWelcomeText"
!define MUI_CUSTOMFUNCTION_GUIINIT DezirClipWelcome

Var DezirClipWelcomeText

Function DezirClipWelcome
  ${Switch} $LANGUAGE
    ${Case} 2052
      StrCpy $DezirClipWelcomeText "为当前 Windows 帐户安装 $(^Name)。$\r$\n$\r$\n安装完成后，可以从「开始」菜单打开应用。"
      ${Break}
    ${Case} 1028
      StrCpy $DezirClipWelcomeText "為目前的 Windows 帳戶安裝 $(^Name)。$\r$\n$\r$\n安裝完成後，可以從「開始」功能表開啟應用程式。"
      ${Break}
    ${Default}
      StrCpy $DezirClipWelcomeText "Install $(^Name) for your Windows account.$\r$\n$\r$\nAfter setup, open it from the Start menu."
  ${EndSwitch}
FunctionEnd
