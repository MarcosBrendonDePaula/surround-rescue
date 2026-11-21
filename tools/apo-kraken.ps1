# Coloca o APO próprio num dispositivo do Kraken, no lugar do Equalizer APO. Rode como ADMINISTRADOR.
#
#   Fone (padrão):  tools\apo-kraken.ps1
#   Microfone:      tools\apo-kraken.ps1 -Mic
#
# Desfazer:
#   Fone:           tools\apo-teste-desfazer.ps1 -Endpoint '{e59c3ff3-fcd2-454f-9738-b402bc3a0801}'
#   Microfone:      tools\apo-teste-desfazer.ps1 -Endpoint '{b773f106-c861-46c0-85ec-b5d91581a674}'
#
# Antes: no app, Configurações → ligue "Usar o motor próprio" (deixa o Equalizer APO em espera,
# senão o efeito sai em dobro).
#
# Posições usadas (dispositivos com o esquema antigo de efeitos, como o Kraken):
#   fone      → pós-mix (GFX, ",2"): uma instância só, depois da mistura de todos os programas
#   microfone → pré-mix (LFX, ",1"): é onde o Equalizer APO estava no microfone

param(
  [switch]$Mic,
  [string]$Endpoint
)
$ErrorActionPreference = 'Stop'
if ($Mic) {
  $flow = 'Capture'; $slot = '{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},1'
  if (-not $Endpoint) { $Endpoint = '{b773f106-c861-46c0-85ec-b5d91581a674}' }
} else {
  $flow = 'Render'; $slot = '{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},2'
  if (-not $Endpoint) { $Endpoint = '{e59c3ff3-fcd2-454f-9738-b402bc3a0801}' }
}
$repo  = Split-Path $PSScriptRoot
$src   = Join-Path $repo 'target\release\sr_apo.dll'
$dst   = 'C:\Program Files\SurroundRescue\apo'
$clsid = '{5A3E1F0C-6B2D-4C8E-9F17-53A2D0B7E4C1}'
$eng   = 'C:\ProgramData\SurroundRescue\engine'
$bak   = Join-Path $eng "backup-$($Endpoint.ToLower()).json"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw 'Abra o PowerShell como administrador.'
}
New-Item -ItemType Directory -Force $dst, $eng | Out-Null
icacls $eng /grant '*S-1-5-19:(OI)(CI)M' '*S-1-5-32-545:(OI)(CI)M' | Out-Null
Stop-Service Audiosrv -Force
Copy-Item $src "$dst\sr_apo.dll" -Force
& regsvr32.exe /s "$dst\sr_apo.dll"

# Essas chaves só permitem "consultar/gravar valor" (nem administrador tem acesso total)
$sub = "SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\$flow\$Endpoint\FxProperties"
$key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($sub, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, [Security.AccessControl.RegistryRights]'QueryValues, SetValue')
if (-not $key) { Start-Service Audiosrv; throw "Dispositivo não encontrado: $flow $Endpoint" }
$cur = $key.GetValue($slot)
if (-not (Test-Path $bak)) {
  @{ endpoint = $Endpoint; flow = $flow; key = $slot; existed = ($null -ne $cur); value = @($cur); kind = 'String' } | ConvertTo-Json | Set-Content $bak -Encoding utf8
}
$key.SetValue($slot, $clsid, [Microsoft.Win32.RegistryValueKind]::String)
$key.Close()
Start-Service Audiosrv
"OK. $flow ${slot}: $cur -> $clsid"
"Backup: $bak"
