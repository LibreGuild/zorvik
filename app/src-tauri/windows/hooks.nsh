; Zorvik NSIS installer hooks (bundle.windows.nsis.installerHooks).
; The install folder holds zorvik.exe, the command-line tool that AI agents
; run (`zorvik mcp`): put it on the user's PATH, and take it off on uninstall.
; The installer is per user (HKCU), so this needs no admin rights.
;
; NSIS strings end at NSIS_MAX_STRLEN (1024): a longer PATH can't be read or
; written whole, so it is left alone (with a note) rather than cut short.
!include "LogicLib.nsh"
!include "WinMessages.nsh"
!include "WordFunc.nsh"

!macro ZORVIK_ENV_CHANGED
  ; Tell open programs (Explorer, new terminals) that PATH changed.
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

; $0 = the user's PATH; the error flag is set when it exists but is too long to read.
!macro ZORVIK_READ_PATH
  ClearErrors
  ReadRegStr $0 HKCU "Environment" "Path"
  ${If} ${Errors}
    ; Missing (fine: start empty) or too long (keep the error flag).
    StrCpy $2 0
    ${Do}
      ClearErrors
      EnumRegValue $3 HKCU "Environment" $2
      ${If} ${Errors}
      ${OrIf} $3 == ""
        StrCpy $0 ""
        ClearErrors
        ${Break}
      ${EndIf}
      ${If} $3 == "Path"
        SetErrors
        ${Break}
      ${EndIf}
      IntOp $2 $2 + 1
    ${Loop}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  !insertmacro ZORVIK_READ_PATH
  ${If} ${Errors}
    DetailPrint "Your PATH is too long to change safely: add $INSTDIR to it to use zorvik in a terminal."
  ${Else}
    ${WordAdd} "$0" ";" "+$INSTDIR" $1
    StrLen $2 $1
    ${If} $2 >= ${NSIS_MAX_STRLEN}
      DetailPrint "Your PATH is too long to change safely: add $INSTDIR to it to use zorvik in a terminal."
    ${ElseIf} $1 != $0
      WriteRegExpandStr HKCU "Environment" "Path" "$1"
      !insertmacro ZORVIK_ENV_CHANGED
    ${EndIf}
  ${EndIf}
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  !insertmacro ZORVIK_READ_PATH
  ${IfNot} ${Errors}
    ${un.WordAdd} "$0" ";" "-$INSTDIR" $1
    ${If} $1 != $0
      WriteRegExpandStr HKCU "Environment" "Path" "$1"
      !insertmacro ZORVIK_ENV_CHANGED
    ${EndIf}
  ${EndIf}
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend
