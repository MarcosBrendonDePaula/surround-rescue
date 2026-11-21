# Tira o processador Nahimic embutido no driver do Kraken do caminho do som.
# O Equalizer APO repassa o áudio ao APO original do driver (Nahimic CVHTAPOLFX), que fica
# configurado com surround/graves/compressor fixos quando o Razer Surround está parado → som estourado.
# Rode como ADMINISTRADOR. Desfazer: tools\nahimic-desligar.ps1 -Voltar
param([switch]$Voltar, [string]$Endpoint = '{e59c3ff3-fcd2-454f-9738-b402bc3a0801}')
$ErrorActionPreference = 'Stop'
$k = "HKLM:\SOFTWARE\EqualizerAPO\Child APOs\$Endpoint"
$bak = "C:\ProgramData\SurroundRescue\engine\nahimic-backup-$($Endpoint.ToLower()).json"
if ($Voltar) {
  $b = Get-Content $bak -Raw | ConvertFrom-Json
  Set-ItemProperty -LiteralPath $k -Name PreMixChild -Value $b.pre
  Set-ItemProperty -LiteralPath $k -Name PostMixChild -Value $b.post
  Remove-Item $bak
  "Nahimic de volta no caminho"
} else {
  $cur = Get-ItemProperty -LiteralPath $k
  if (-not (Test-Path $bak)) { @{ pre = $cur.PreMixChild; post = $cur.PostMixChild } | ConvertTo-Json | Set-Content $bak -Encoding utf8 }
  Set-ItemProperty -LiteralPath $k -Name PreMixChild -Value ''
  "Nahimic fora do caminho (antes: pré-mix $($cur.PreMixChild))"
}
Restart-Service Audiosrv -Force
