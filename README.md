# Surround Rescue

Motor de áudio para Windows 10/11: surround virtual, equalizador, plugins VST3 e um **editor de fluxo visual** para rotear e processar o som de qualquer programa, microfone ou saída.

Nasceu para substituir o Razer Surround (descontinuado) em um headset Razer Kraken 7.1 Chroma — e virou um sistema genérico, que funciona com qualquer dispositivo de áudio.

> **Projeto independente.** Não tem nenhuma ligação com a Razer Inc. nem com a Nahimic/A-Volute. "Razer", "Kraken" e "Nahimic" são marcas dos seus donos e aparecem aqui só para descrever compatibilidade e o problema original.

> **Estado: experimental.** Funciona e foi testado em uso real, mas mexe no caminho de áudio do Windows. Leia "Como desfazer" antes de instalar.

## O que faz

- **Motor próprio dentro do Windows** (APO): o efeito roda no caminho de áudio do dispositivo, para todos os programas, com **latência zero** somada.
- **Anti-estouro sempre ligado**: limitador a −1 dBFS no fim da cadeia.
- **Surround virtual, equalizador de 10 bandas, graves, clareza de voz, normalização** (compressor), perfis e troca automática de perfil por programa.
- **Plugins VST3** em qualquer dispositivo, com a janela do plugin, num executor separado: se um plugin travar, o som passa direto.
- **Editor de fluxo** (estilo editor de nós): caixas e setas, mesa de som com fader e mudo por entrada, blocos nativos (ganho, inverter/subtrair, pan, mono, atraso, filtros).
- **Fluxos com triggers**: o som de um **programa** (captura por processo), de um **microfone** ou de **uma saída** inteira, enviado para **qualquer outro dispositivo** — com conversão de taxa, canais e relógio. Condição "só quando o programa X estiver aberto".
- **Microfone**: colocar o som de um programa dentro do microfone (ex.: música no Discord), em qualquer microfone com o motor instalado.
- **Medições**: CPU por fluxo, tempo por bloco, atraso de cada envio/captura.

## Como funciona

```
programas / jogos / microfones
        │                (captura por processo · receber de dispositivo · ouvir uma saída)
        ▼
APO próprio no Windows ──canal (memória compartilhada)──▶ executor (sr-host)
  latência zero, anti-estouro      se o executor atrasar,      plugins VST3, fluxo, blocos,
                                   o som passa direto          envios para outros dispositivos
        │
        ▼
dispositivo principal                      outros dispositivos (ponte universal: taxa/canais/relógio)
```

| Pasta | O que é |
|---|---|
| `crates/sr-dsp` | motor de áudio (surround, EQ, compressor, limitador), sem alocação no tempo real |
| `crates/sr-apo` | o APO (objeto COM carregado pelo `audiodg.exe`), com suporte a agregação |
| `crates/sr-ipc` | canal APO ↔ executor (memória compartilhada + eventos, com prazo por bloco) |
| `crates/sr-host` | executor: VST3, grafo do fluxo, blocos, envios, capturas, fluxos com triggers, medições |
| `crates/sr-bridge` | ponte universal entre dispositivos (resampler sinc + correção de relógio) |
| `crates/sr-vst` | o próprio Surround Rescue como plugin VST3/CLAP |
| `src-tauri`, `src` | o app (Tauri 2 + Rust, interface em HTML/JS) |
| `tools` | scripts de instalação/teste do APO e de desfazer |
| `docs` | estudo do Razer Surround original e o desenho do projeto |

## Compilar

Requer Rust (stable), Node.js e Windows 10 2004+ (a captura por processo depende dele).

```
npm install
cargo build --release                          # app, executor, APO
cargo xtask bundle sr-vst --release            # plugin VST3/CLAP
cargo test --workspace                         # testes
npm run build                                  # instalador (NSIS)
```

O instalador inclui o [Equalizer APO](https://sourceforge.net/projects/equalizerapo/) (GPL-2.0), que continua disponível como motor alternativo. Coloque o instalador dele em `src-tauri/resources/EqualizerAPO-x64.exe` antes de gerar o pacote (não é versionado aqui).

## Instalar o motor num dispositivo (experimental)

Precisa de administrador. Os scripts guardam o valor original e reiniciam só o áudio do Windows (sem reiniciar o PC).

```
tools\apo-kraken.ps1 -Endpoint '{guid-da-saida}'          # saída (pós-mix)
tools\apo-kraken.ps1 -Mic -Endpoint '{guid-do-microfone}' # microfone (pré-mix)
tools\apo-teste-instalar.ps1 -Endpoint '{guid}'           # saídas com cadeia moderna de efeitos (EFX)
```

Os `{guid}` aparecem no app em Configurações e na aba Fluxo.

### Como desfazer

```
tools\apo-teste-desfazer.ps1 -Endpoint '{guid}'
```

Restaura o valor original da posição de efeito, reinicia o áudio e, se nenhum dispositivo usar mais o motor, desregistra o APO.

## Licença

[GPL-3.0](LICENSE). O plugin VST3 usa bibliotecas sob GPL-3.0 e o instalador inclui o Equalizer APO (GPL-2.0, sem modificações — ver `src-tauri/resources/THIRD-PARTY.txt`).
