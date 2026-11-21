# Desfaz o teste do APO num dispositivo: devolve o valor original da posição de efeito,
# reinicia o áudio e, se nenhum outro dispositivo usa o APO, desregistra.
# Rode como ADMINISTRADOR.
#   Realtek (padrão): tools\apo-teste-desfazer.ps1
#   Kraken:           tools\apo-teste-desfazer.ps1 -Endpoint '{e59c3ff3-fcd2-454f-9738-b402bc3a0801}'
param([string]$Endpoint = '{15a6a387-82c2-419b-b0b0-b472feaaa4e6}')
$ErrorActionPreference = 'Stop'
$eng = 'C:\ProgramData\SurroundRescue\engine'
$bak = Join-Path $eng "backup-$($Endpoint.ToLower()).json"
$dll = 'C:\Program Files\SurroundRescue\apo\sr_apo.dll'

if (Test-Path $bak) {
  $b = Get-Content $bak -Raw | ConvertFrom-Json
  # Essas chaves só permitem "consultar/gravar valor" (nem administrador tem acesso total)
  $sub = "SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\$($b.flow)\$($b.endpoint)\FxProperties"
  $rights = [Security.AccessControl.RegistryRights]'QueryValues, SetValue'
  $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($sub, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, $rights)
  if ($b.existed -and $b.kind -eq 'String') {
    $key.SetValue($b.key, [string]@($b.value)[0], [Microsoft.Win32.RegistryValueKind]::String)
  } elseif ($b.existed) {
    $key.SetValue($b.key, [string[]]@($b.value), [Microsoft.Win32.RegistryValueKind]::MultiString)
  } else {
    $key.DeleteValue($b.key, $false)
  }
  $key.Close()
  Remove-Item $bak
  "Valor original restaurado em $($b.endpoint)"
} else {
  "Nenhum backup para $Endpoint (nada a desfazer)"
}
Restart-Service Audiosrv -Force
if ((Test-Path $dll) -and -not (Get-ChildItem $eng -Filter 'backup-*.json')) {
  & regsvr32.exe /s /u $dll
  "APO desregistrado (nenhum dispositivo o usa mais)"
}
