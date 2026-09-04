; ============================================================================
;  QookiX Launcher NSIS installer/uninstaller hooks
;  Learned from Mystic-Stars/Axolotl's hooks.nsi.
;  These macros are invoked by the custom installer.nsi via !insertmacro:
;    NSIS_HOOK_PREINSTALL / NSIS_HOOK_POSTINSTALL / NSIS_HOOK_PREUNINSTALL
; ============================================================================
Var /GLOBAL QXOldInstallDir

; ----------------------------------------------------------------------------
;  PREINSTALL: close any running instance so files aren't locked during an
;  upgrade, and remember the previous install location to repair shortcuts.
; ----------------------------------------------------------------------------
!macro NSIS_HOOK_PREINSTALL
  ; Best-effort: terminate a running QookiX before overwriting its binaries.
  ; taskkill is built into Windows; failure (no process) is non-fatal.
  ExecWait 'taskkill /im "${MAINBINARYNAME}.exe" /f' $0
  ; Remember previous install location (stored quoted in the registry) so the
  ; post-install step can fix shortcuts that still point to the old path.
  ReadRegStr $QXOldInstallDir SHCTX "${UNINSTKEY}" "InstallLocation"
  StrCpy $QXOldInstallDir $QXOldInstallDir "" 1
  StrCpy $QXOldInstallDir $QXOldInstallDir -1 ""
!macroend

; ----------------------------------------------------------------------------
;  POSTINSTALL: if the desktop shortcut still points at the previous install
;  directory, redirect it to the new one (happens on upgrades).
; ----------------------------------------------------------------------------
!macro NSIS_HOOK_POSTINSTALL
  !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$QXOldInstallDir\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  ${EndIf}
  ; --- qookix:// protocol registration (runs on every install AND upgrade) ---
  ; The template's deep_link_protocols loop is not reliably executed on
  ; silent in-app upgrades (observed: 0.5.4 -> 0.5.5 lost the association),
  ; so register it explicitly here as belt-and-braces. Idempotent: each run
  ; overwrites with identical values, so upgrades stay correct after the
  ; install dir changes.
  WriteRegStr SHCTX "Software\Classes\qookix" "URL Protocol" ""
  WriteRegStr SHCTX "Software\Classes\qookix" "" "URL:qookix protocol"
  WriteRegStr SHCTX "Software\Classes\qookix\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "Software\Classes\qookix\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
!macroend

; ----------------------------------------------------------------------------
;  PREUNINSTALL: when the user opts to delete app data, safely remove any
;  reparse points (junctions / symlinks) under the data dirs before deleting,
;  so we don't leave behind inaccessible virtual directories.
; ----------------------------------------------------------------------------
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ${If} ${FileExists} "$APPDATA\${BUNDLEID}"
      Push "$APPDATA\${BUNDLEID}"
      Call un.RemoveReparsePoints
    ${EndIf}
    ${If} ${FileExists} "$LOCALAPPDATA\${BUNDLEID}"
      Push "$LOCALAPPDATA\${BUNDLEID}"
      Call un.RemoveReparsePoints
    ${EndIf}
  ${EndIf}
!macroend

Function un.RemoveReparsePoints
  Exch $0 ; root directory to scan
  Push $1 ; FindFirst handle
  Push $2 ; current entry name
  Push $3 ; current entry path
  Push $4 ; entry attributes
  Push $5 ; spare
  FindFirst $1 $2 "$0\*"
  ${If} ${Errors}
    Goto done
  ${EndIf}
  loop:
    StrCmp $2 "." next
    StrCmp $2 ".." next
    StrCpy $3 "$0\$2"
    System::Call 'kernel32::GetFileAttributes(t r3) i.r4'
    IntOp $4 $4 & 0x400 ; FILE_ATTRIBUTE_REPARSE_POINT
    IntCmp $4 0 notReparse isReparse isReparse
    isReparse:
      System::Call 'kernel32::GetFileAttributes(t r3) i.r4'
      IntOp $4 $4 & 0x10 ; FILE_ATTRIBUTE_DIRECTORY
      IntCmp $4 0 removeFileLink removeDirLink removeDirLink
      removeDirLink:
        RmDir "$3" ; removes the junction / directory symlink itself
        Goto next
      removeFileLink:
        Delete "$3" ; removes the file symlink itself
        Goto next
    notReparse:
      System::Call 'kernel32::GetFileAttributes(t r3) i.r4'
      IntOp $4 $4 & 0x10
      IntCmp $4 0 next recurse recurse
      recurse:
        Push $3
        Call un.RemoveReparsePoints
    next:
      FindNext $1 $2
      IfErrors done
      Goto loop
  done:
    FindClose $1
    Pop $5
    Pop $4
    Pop $3
    Pop $2
    Pop $1
    Pop $0
FunctionEnd
