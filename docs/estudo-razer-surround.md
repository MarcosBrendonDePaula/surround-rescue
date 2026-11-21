# Estudo do Razer Surround 1.05.28 (instalação atual)

Data: 2026-11-21 — Windows 11 Pro 26200, headset Razer Kraken 7.1 Chroma (USB VID_1532 PID_0504).

## Problema relatado
- Discord (e outros) não entram no processamento do Surround.
- É preciso trocar o dispositivo no Discord várias vezes até entrar.
- Enquanto isso o áudio fica estourado/alto/sem controle; o serviço parece travado.

## Componentes instalados
| Arquivo | Papel |
|---|---|
| `Razer Surround\Driver\RzSurroundVAD.sys` | Driver de áudio virtual (WDM/KS), DriverVer 10/13/2016 15.57.36.635, assinado |
| `Razer Surround\Driver\RzSurroundVADStreamingService.exe` | Serviço `RzSurroundVADStreamingService` (Automático) que faz o processamento |
| `Driver\amd64\RzSurroundVADAudioDeviceManager.dll` | COM `IAudioDeviceManagerCOM`: troca dispositivo padrão, liga/desliga SysFx, `SetRenderDeviceVisibility` |
| `Driver\amd64\RzSurroundVADSettingsIPC.dll` / `...SettingsStreamROT.dll` | IPC de configurações entre UI e serviço |

## Achados
1. **Motor é Nahimic (A-Volute)**: strings `NahimicVADStreamingService.exe`, `NahimicVHT2`; PDB `C:\GitBucket\vad_surround\Surround_1_01_66\...`.
   Configurações em `HKLM\SOFTWARE\Razer\Kraken71Chroma_1.0.0\NahimicSettings` (por aplicativo, chaves `kNhSet_*`).
2. **Pipeline**: app → endpoint virtual "Alto-falantes (Razer Surround Audio Controller)" → serviço captura via Kernel Streaming (pins KS + IOCTL, PortAudio) → NahimicVHT2 → escreve no Kraken também via KS, fora do mixer do Windows.
   Isso provoca disputa pelo Kraken quando um app abre o headset diretamente.
3. **Endpoint virtual oculto**: DeviceState `0x10000001` (ativo + oculto) — por isso não aparece no painel de som.
4. **Histórico de sessões por app** em
   `HKCU\Software\Microsoft\Internet Explorer\LowRegistry\Audio\PolicyConfig\PropertyStore`:
   as entradas só têm o valor padrão `{2}.<endpoint>|<exe>%b{GUID}` — são registros de "este app tocou neste dispositivo", **não** regras forçadas
   (correção de uma leitura anterior). Mesmo assim mostram o padrão do problema: em praticamente todos os apps (jogos, navegadores, Discord, Telegram…)
   a maioria das sessões foi aberta direto no Kraken (`usb#vid_1532&pid_0504`) e só raras no Surround (`root#media`).
   Ou seja, o problema é geral: o Surround falha em manter o dispositivo virtual como destino padrão, e os apps acabam no Kraken cru.
   (Em 2026-11-21 as 56 entradas do Discord foram removidas; backup em `dumps/`, fora do git.)
5. Log do serviço (`ProgramData\Razer\Synapse\Logs\RzSurround\RzSurroundVADStreamingService.log`) vazio — sem diagnóstico de travamento.

## Caminhos avaliados
- **A — Vigia**: corrige regras por app, reinicia serviço travado, corta volume se áudio sair cru. Mantém o som Nahimic.
- **B — Reescrever o serviço** sobre o driver da Razer: inviável/caro (motor Nahimic fechado).
- **C — Efeito direto no Kraken** (Equalizer APO + HRTF tipo HeSuVi + painel próprio): elimina o roteamento por completo.

Decisão: pendente.
