# surround-rescue

Substituto/correção para o Razer Surround (descontinuado) no Windows 11, para o headset Razer Kraken 7.1 Chroma.

- [Estudo da instalação atual](docs/estudo-razer-surround.md)

> Binários e dados da Razer/Nahimic não são versionados aqui (uso pessoal, propriedade de terceiros).

## App (v0.1)
Tauri 2 + Rust. Gera a configuração do [Equalizer APO](https://sourceforge.net/projects/equalizerapo/) direto na saída do Kraken:
espacialização (crossfeed), largura estéreo, graves, clareza de voz, agudos e volume com **anti-estouro automático**.

Pré-requisito: Equalizer APO instalado e ligado no "Fone de ouvido do headset (Razer Kraken 7.1 Chroma)".

```
npm install
npm run dev      # desenvolvimento
npm run build    # instalador em src-tauri/target/release/bundle/nsis/
cd src-tauri && cargo test
```
Design: [docs/superpowers/specs/2026-11-21-surround-rescue-app-design.md](docs/superpowers/specs/2026-11-21-surround-rescue-app-design.md)
