; NSIS hooks for vsplit: add per-user install directory (%LOCALAPPDATA%\vsplit)
; to HKCU\Environment\Path on install, remove on uninstall, and broadcast WM_SETTINGCHANGE.

!include "LogicLib.nsh"
!include "WinMessages.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $0 HKCU "Environment" "Path"
  StrCmp $0 "" _vsplit_set_path
  ; Avoid duplicate PATH entries
  Push $0
  Push "$INSTDIR"
  Call vsplit_StrContains
  Pop $1
  StrCmp $1 "" 0 _vsplit_done_add
  WriteRegExpandStr HKCU "Environment" "Path" "$0;$INSTDIR"
  Goto _vsplit_broadcast_add
_vsplit_set_path:
  WriteRegExpandStr HKCU "Environment" "Path" "$INSTDIR"
_vsplit_broadcast_add:
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=2000
_vsplit_done_add:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ReadRegStr $0 HKCU "Environment" "Path"
  StrCmp $0 "" _vsplit_done_rem
  ; Remove ";$INSTDIR" or "$INSTDIR;" or exact "$INSTDIR"
  Push $0
  Push ";$INSTDIR"
  Push ""
  Call un.vsplit_StrRep
  Pop $0
  Push $0
  Push "$INSTDIR;"
  Push ""
  Call un.vsplit_StrRep
  Pop $0
  StrCmp $0 "$INSTDIR" 0 +2
  StrCpy $0 ""
  WriteRegExpandStr HKCU "Environment" "Path" "$0"
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=2000
_vsplit_done_rem:
!macroend

Function vsplit_StrContains
  Exch $R1 ; needle
  Exch
  Exch $R2 ; haystack
  Push $R3
  Push $R4
  Push $R5
  StrLen $R3 $R1
  StrCpy $R4 0
  loop:
    StrCpy $R5 $R2 $R3 $R4
    StrCmp $R5 $R1 found
    StrCmp $R5 "" notfound
    IntOp $R4 $R4 + 1
    Goto loop
  found:
    StrCpy $R1 $R5
    Goto done
  notfound:
    StrCpy $R1 ""
  done:
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Exch $R1
FunctionEnd

Function un.vsplit_StrRep
  Exch $R2 ; replacement
  Exch 1
  Exch $R1 ; search
  Exch 2
  Exch $R0 ; input
  Push $R3
  Push $R4
  Push $R5
  Push $R6
  Push $R7
  Push $R8
  Push $R9
  StrCpy $R3 0
  StrLen $R4 $R1
  StrLen $R6 $R0
  StrLen $R9 $R2
  loop:
    StrCpy $R5 $R0 $R4 $R3
    StrCmp $R5 $R1 found
    StrCmp $R3 $R6 done
    IntOp $R3 $R3 + 1
    Goto loop
  found:
    StrCpy $R7 $R0 $R3
    IntOp $R8 $R3 + $R4
    StrCpy $R8 $R0 "" $R8
    StrCpy $R0 "$R7$R2$R8"
    IntOp $R3 $R3 + $R9
    StrLen $R6 $R0
    Goto loop
  done:
    Pop $R9
    Pop $R8
    Pop $R7
    Pop $R6
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Pop $R1
    Exch $R0
FunctionEnd
