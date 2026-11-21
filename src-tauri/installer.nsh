; Instala o motor de áudio (Equalizer APO) em silêncio se ainda não existir.
!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$PROGRAMFILES64\EqualizerAPO\EqualizerAPO.dll" eqapo_ok 0
    DetailPrint "Instalando o motor de áudio (Equalizer APO)..."
    ExecWait '"$INSTDIR\resources\EqualizerAPO-x64.exe" /S' $0
    DetailPrint "Motor de áudio: código $0"
  eqapo_ok:
!macroend
