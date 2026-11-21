# Surround Rescue — app v1 (design)

## Intenção
Substituir o Razer Surround (descontinuado, motor Nahimic) no Kraken 7.1 Chroma, acabando com:
travamento do serviço, áudio estourado/cru e programas que não "entram" no processamento.

## Abordagens consideradas
- A — vigia ao lado do Razer Surround: remendo; o defeito (apps caindo no Kraken cru) é geral.
- B — reescrever o serviço Razer: motor Nahimic fechado, semanas de RE.
- **C (escolhida)** — processamento direto na saída real do Kraken via Equalizer APO (APO oficial do Windows), com app próprio.
  Sem driver, sem dispositivo virtual, sem serviço disputando o fone.

## Decisões
- Usuário: caminho C, Tauri + Rust.
- Kraken é endpoint **estéreo** 48 kHz/16 bit → v1 = "estéreo para 3D" (crossfeed + alargamento). 7.1 real via dispositivo virtual opcional fica para v2 (decisão do usuário: começar pela base).
- Minhas decisões (corrigíveis):
  - Frontend HTML/JS puro (sem framework) para manter o app pequeno.
  - O app escreve `config\surround-rescue.txt` e faz `config.txt` apenas incluí-lo (backup do original em `config.txt.surround-rescue.bak`).
  - **Anti-estouro**: preamp automático = −(maior ganho positivo somado) − margem, sempre como primeiro passo. Limitador real (VST) fica para v2.
  - Configurações do usuário em `%APPDATA%\surround-rescue\settings.json`.
  - Status no app: Equalizer APO instalado? registrado no Kraken? serviço Razer parado? (conflito).

## v1 — recursos
- Liga/desliga geral.
- Perfis: Natural, Jogo, Filme, Música, Personalizado.
- Controles: espacialização (crossfeed), largura estéreo, graves, clareza de voz, agudos, volume.
- Ícone na bandeja (mostrar, ligar/desligar, sair).

## Fora do v1
7.1 real, perfis HRIR (convolução), perfil por programa, microfone (gate/redução de ruído), VU meter, auto-reparo após update do Windows.
