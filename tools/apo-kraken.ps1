# Coloca o APO próprio no FONE DO KRAKEN (posição pós-mix / GFX), no lugar do Equalizer APO.
# Rode como ADMINISTRADOR. Desfazer: tools\apo-teste-desfazer.ps1 -Endpoint '{e59c3ff3-fcd2-454f-9738-b402bc3a0801}'
#
# Antes: no app, Configurações → ligue "Usar o motor próprio" (deixa o Equalizer APO em passa direto,
# senão o efeito sai em dobro).

param([string]$Endpoint = '{e59c3ff3-fcd2-454f-9738-b402bc3a0801}')
$ErrorActionPreference = 'Stop'
$repo  = Split-Path $PSScriptRoot
$src   = Join-Path $repo 'target\release\sr_apo.dll'
$dst   = 'C:\Program Files\SurroundRescue\apo'
$clsid = '{5A3E1F0C-6B2D-4C8E-9F17-53A2D0B7E4C1}'
$eng   = 'C:\ProgramData\SurroundRescue\engine'
$gfx   = '{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},2'
$bak   = Join-Path $eng "backup-$($Endpoint.ToLower()).json"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw 'Abra o PowerShell como administrador.'
}
New-Item -ItemType Directory -Force $dst, $eng | Out-Null
icacls $eng /grant '*S-1-5-19:(OI)(CI)M' '*S-1-5-32-545:(OI)(CI)M' | Out-Null
Stop-Service Audiosrv -Force
Copy-Item $src "$dst\sr_apo.dll" -Force
& regsvr32.exe /s "$dst\sr_apo.dll"

$sub = "SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$Endpoint\FxProperties"
$key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($sub, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, [Security.AccessControl.RegistryRights]'QueryValues, SetValue')
if (-not $key) { Start-Service Audiosrv; throw "Dispositivo não encontrado: $Endpoint" }
$cur = $key.GetValue($gfx)
if (-not (Test-Path $bak)) {
  @{ endpoint = $Endpoint; flow = 'Render'; key = $gfx; existed = ($null -ne $cur); value = @($cur); kind = 'String' } | ConvertTo-Json | Set-Content $bak -Encoding utf8
}
$key.SetValue($gfx, $clsid, [Microsoft.Win32.RegistryValueKind]::String)
$key.Close()
Start-Service Audiosrv
"OK. Kraken pós-mix: $cur -> $clsid"
"Backup: $bak"
