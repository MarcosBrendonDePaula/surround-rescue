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

## v0.2 (feito)
Interface no estilo do Razer Surround (abas CALIBRAÇÃO, ÁUDIO, EQUALIZADOR + MICROFONE, PERFIS, CONFIGURAÇÕES),
perfis nomeados, troca automática por programa em foco, microfone (reforço, ganho, filtro de graves, nitidez),
iniciar com o Windows, configuração separada por dispositivo (`Device:` com GUID do fone e do microfone).

## Fora do v1
7.1 real, perfis HRIR (convolução), perfil por programa, microfone (gate/redução de ruído), VU meter, auto-reparo após update do Windows.

## v0.4 (feito)
- Qualquer dispositivo de saída/entrada (lista do Windows), não só o Kraken.
- Ligação do Equalizer APO pelo próprio app (`--apo install|uninstall Render|Capture {id}`, elevado via UAC),
  reproduzindo o esquema "Child APOs" do Configurator 1.4 (validado contra o registro real do Kraken).
  Dispositivos que só usam SFX/MFX ainda exigem o Configurator.
- Instalador único (NSIS perMachine) com o Equalizer APO 1.4.2 embutido, instalado em silêncio se faltar (GPL-2, ver THIRD-PARTY.txt).

## Descoberta
O driver do Kraken trazia um APO da Nahimic (`{29dc9ab1-...}` CVHTAPOLFX) na posição LFX — a Razer usava APO dentro do driver assinado do headset,
além do driver virtual do Razer Surround.

## Futuro: APO próprio
Motivo: compressor/limitador real (normalização), mudança instantânea sem recarregar arquivo, VU meter, HRTF próprio.
A lógica de áudio (`eqapo.rs`) já está isolada do motor para permitir a troca.

## APO próprio — primeiro carregamento real (2026-11-21)
- audiodg cria o APO **agregado** (CreateInstance com outer, pedindo IUnknown) → objeto COM escrito à mão.
- Em dispositivos modernos (Realtek) o APO entra no fim da cadeia composta EFX (`{d04e05a6-...},15`, REG_MULTI_SZ),
  sem tirar o efeito do fabricante. Chaves FxProperties: admins só têm QueryValues/SetValue.
- Validado na Realtek Digital Output: Initialize → GUID do endpoint → LockForProcess 48 kHz/2 ch → prova de vida.
- Interfaces opcionais consultadas pelo audiodg (respondemos E_NOINTERFACE): IAudioSystemEffects2/3,
  IAudioProcessingObjectNotifications, IAudioProcessingObjectLoggingService etc. — candidatas a implementar depois.

## Etapas 2 e 3 (2026-11-21, manhã)
- `sr-ipc`: canal APO↔executor. Só serviços criam objetos `Global\`: o APO cria, o executor conecta.
- `sr-host`: executor de VST3 (biblioteca `vst3-host`, MIT). Cadeia por dispositivo em
  `ProgramData\SurroundRescue\chains\{guid}.json`; plugins carregados fora do atendimento do áudio.
  Ponta a ponta local: 299/300 blocos pelo VST, pior ida e volta 0,13 ms.
- App: aba Plugins (dispositivo, adicionar da varredura ou por caminho, ligar/desligar, reordenar, remover);
  sobe o executor quando há cadeias; grava `engine\{guid}.json` para o APO próprio.
- Falta: janela (editor) do plugin, teste no Kraken com o APO próprio, empacotar sr-apo/sr-host no instalador.

## Editor de fluxo (2026-11-21, tarde)
- Grafo no arquivo de cadeia (`graph`: nós in/out/mixer/plugin + setas com ganho). Ausente = fila de `plugins`.
- Cada nó soma as entradas (com ganho); ordenação topológica; ciclo = fluxo recusado (passa direto). 8 testes.
- Aba Fluxo: arrastar caixas, puxar setas entre portas, volume por seta, mixer, duplo clique abre janela,
  Delete remove, "Organizar em fila". Lista vira só leitura quando o fluxo não é fila.
- Ponta a ponta com fluxo paralelo: processado pelo executor (0,15 ms ida e volta).
- Ainda sem compensação de latência entre ramos (plugins com latência podem defasar caminhos paralelos).

## Motor universal entre dispositivos (2026-11-21)
- `sr-bridge`: interno float 32 estéreo; por destino: canais (mono/estéreo/7.1, downmix ITU), taxa (sinc, rubato 0.16)
  e relógio (controle proporcional sobre o nível médio da fila; integrador removido porque oscilava).
- Testes: 10 min em sincronia com ±100/150 ppm, 48→44,1 kHz mantém tom e volume, correção estável < 0,05%.
- `sr-host/send.rs`: envio WASAPI compartilhado por eventos; nó `send` no grafo; recusa envio que volte ao dispositivo de origem.
- Teste real (Realtek, 30 s em tempo real): 0 faltas durante o uso, correção convergindo +0,04% → +0,01%.
- Falta: nó "Receber de dispositivo" (misturar microfones), medidores de nível.
