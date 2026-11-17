; NSIS refuses to compile an installer whose data block grows past 2 GB, and the engine tree
; alone is 2.7 GB. packaging/fetch-engines.ps1 therefore ships it as one solid engines.7z
; beside the bundled 7z.exe, and this hook unpacks it once the installer has copied every file.
;
; 7-Zip is a plain exe, so no shell is involved; the exit code is checked and the archive is
; deleted only after a successful extraction. A failure aborts the install instead of leaving
; an app that silently cannot convert anything.
!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Unpacking bundled conversion engines (about a minute)..."
  ExecWait '"$INSTDIR\engines\7z.exe" x -y "$INSTDIR\engines\engines.7z" -o"$INSTDIR\engines"' $0
  IntCmp $0 0 engines_unpacked
    MessageBox MB_ICONSTOP|MB_OK "The bundled conversion engines could not be unpacked (7-Zip exit code $0). Please reinstall Zeidex Convert."
    Abort
  engines_unpacked:
  Delete "$INSTDIR\engines\engines.7z"
!macroend
