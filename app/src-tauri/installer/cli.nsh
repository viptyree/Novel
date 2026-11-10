!include nsDialogs.nsh
!include LogicLib.nsh
!include WinMessages.nsh
Var NovelPathCheckbox
Var NovelPathState
; Hook includes are loaded before the installer's standard pages.
Page custom NovelPathPage NovelPathLeave
Function NovelPathPage
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 35u "Sodarie Novel CLI: type novel in PowerShell, CMD or SSH."
  Pop $0
  ${NSD_CreateCheckbox} 0 45u 100% 30u "Add Novel to the current user's PATH (recommended)"
  Pop $NovelPathCheckbox
  ${NSD_Check} $NovelPathCheckbox
  nsDialogs::Show
FunctionEnd
Function NovelPathLeave
  ${NSD_GetState} $NovelPathCheckbox $NovelPathState
FunctionEnd
!macro NSIS_HOOK_PREINSTALL
  IfSilent 0 +2
    StrCpy $NovelPathState 1
!macroend
!macro NSIS_HOOK_POSTINSTALL
  ${If} $NovelPathState == 1
    ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\installer\cli-path.ps1" -Action Add -Directory "$INSTDIR"' $0
    ${If} $0 != 0
      DetailPrint "Novel PATH setup failed. Add $INSTDIR to your user PATH manually."
    ${EndIf}
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
  ${EndIf}
!macroend
!macro NSIS_HOOK_PREUNINSTALL
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\installer\cli-path.ps1" -Action Remove -Directory "$INSTDIR"' $0
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
