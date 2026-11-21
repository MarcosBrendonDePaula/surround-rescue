# Desfaz o teste do APO: devolve a cadeia de efeitos original, remove o registro e reinicia o áudio.
# Rode como ADMINISTRADOR.
param([string]$Endpoint = '{15a6a387-82c2-419b-b0b0-b472feaaa4e6}')
$ErrorActionPreference = 'Stop'
$eng = 'C:\ProgramData\SurroundRescue\engine'
$bak = Join-Path $eng "backup-$($Endpoint.ToLower()).json"
$dll = 'C:\Program Files\SurroundRescue\apo\sr_apo.dll'

if (Test-Path $bak) {
  $b  = Get-Content $bak -Raw | ConvertFrom-Json
  $fx = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\$($b.flow)\$($b.endpoint)\FxProperties"
  if ($b.existed) {
    New-ItemProperty -LiteralPath $fx -Name $b.key -PropertyType MultiString -Value @($b.value) -Force | Out-Null
  } else {
    Remove-ItemProperty -LiteralPath $fx -Name $b.key -ErrorAction SilentlyContinue
  }
  Remove-Item $bak
  "Cadeia original restaurada em $($b.endpoint)"
}
Restart-Service Audiosrv -Force
if (Test-Path $dll) { & regsvr32.exe /s /u $dll; "APO desregistrado" }
