# Teste do APO próprio num dispositivo SEM USO (Realtek Digital Output).
# Rode como ADMINISTRADOR. Desfazer: tools\apo-teste-desfazer.ps1
#
# O que faz:
#  1. copia o sr_apo.dll para C:\Program Files\SurroundRescue\apo (o audiodg não lê a pasta do usuário)
#  2. registra o APO (regsvr32)
#  3. cria C:\ProgramData\SurroundRescue\engine com permissão de leitura/escrita para o serviço de áudio
#  4. ACRESCENTA o APO no fim da cadeia de efeitos de endpoint (EFX) da saída de teste, guardando a original
#  5. reinicia só o áudio do Windows (sem reiniciar o PC)

param(
  [string]$Endpoint = '{15a6a387-82c2-419b-b0b0-b472feaaa4e6}',   # Realtek Digital Output
  [string]$Flow = 'Render'
)
$ErrorActionPreference = 'Stop'
$repo  = Split-Path $PSScriptRoot
$src   = Join-Path $repo 'target\release\sr_apo.dll'
$dst   = 'C:\Program Files\SurroundRescue\apo'
$clsid = '{5A3E1F0C-6B2D-4C8E-9F17-53A2D0B7E4C1}'
$eng   = 'C:\ProgramData\SurroundRescue\engine'
$efx   = '{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},15'
$bak   = Join-Path $eng "backup-$($Endpoint.ToLower()).json"

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw 'Abra o PowerShell como administrador.'
}
if (-not (Test-Path $src)) { throw "Compile antes: cargo build -p sr-apo --release ($src)" }

New-Item -ItemType Directory -Force $dst, $eng | Out-Null
icacls $eng /grant '*S-1-5-19:(OI)(CI)M' '*S-1-5-32-545:(OI)(CI)M' | Out-Null   # LocalService e Usuários
Copy-Item $src "$dst\sr_apo.dll" -Force
& regsvr32.exe /s "$dst\sr_apo.dll"
if ($LASTEXITCODE -ne 0) { throw "regsvr32 falhou ($LASTEXITCODE)" }

# Essas chaves só permitem "consultar/gravar valor" (nem administrador tem acesso total):
# abrimos pedindo exatamente isso.
$sub = "SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\$Flow\$Endpoint\FxProperties"
$rights = [Security.AccessControl.RegistryRights]'QueryValues, SetValue'
$key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($sub, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, $rights)
if (-not $key) { throw "Dispositivo não encontrado: $Endpoint" }
$cur = $key.GetValue($efx)
if (-not (Test-Path $bak)) {
  @{ endpoint = $Endpoint; flow = $Flow; key = $efx; existed = ($null -ne $cur); value = @($cur) } | ConvertTo-Json | Set-Content $bak -Encoding utf8
}
$list = @($cur | Where-Object { $_ -and $_ -ne $clsid }) + $clsid
$key.SetValue($efx, [string[]]$list, [Microsoft.Win32.RegistryValueKind]::MultiString)
$key.Close()

Restart-Service Audiosrv -Force
"OK. Cadeia EFX agora: $($list -join ' -> ')"
"Backup: $bak"
