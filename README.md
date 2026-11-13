# DISC2 — Transmissão de Tela Nativa Ultra-Leve em Rust (1080p 60fps)

Aplicativo desktop **100% Rust puro nativo** (`egui` + OpenGL + Win32 GDI/DWM) para Windows, focado em transmissão de tela e jogos em **1080p 60fps** (com redução em tempo real para `720p`, `480p`, `360p` e `30/15 FPS`), consumindo apenas **~15–25 MB de RAM** e zero navegador/Electron.

## Destaques de Leveza e Performance

- **100% Nativo em Rust (Sem Electron / Sem Node.js)**: executável único (`DISC2.exe`) de poucos megabytes, renderizado direto na GPU via OpenGL (`eframe`/`glow`).
- **Qualidade ajustável em tempo real**: alterne entre `1080p (Full HD)`, `720p (HD Leve)`, `480p (SD Rápido)` e `360p (Ultra Leve)`, além de `60 FPS`, `30 FPS` e `15 FPS` e nível de nitidez (`90%` a `45%`) durante a transmissão sem interromper a live.
- **Captura de Tela Inteira ou Janelas/Jogos**: enumeração nativa de monitores e janelas do Windows (`EnumDisplayMonitors` / `EnumWindows` / `PrintWindow` com suporte a `PW_RENDERFULLCONTENT`).
- **Conexão Direta App-para-App**:
  - **Descoberta Automática na Rede / VPN (LAN, Radmin VPN, ZeroTier, Tailscale)**: salas ativas aparecem automaticamente na barra lateral com 1 clique para assistir.
  - **Conexão Direta por IP**: basta copiar o endereço da sala e colar no aplicativo.
- **Modo Jogo (Ocultar Prévia Local)**: desativa a prévia local na sua tela enquanto joga, reduzindo o custo de renderização local a praticamente zero.

## Como Executar

- Dê dois cliques em **`DISC2.exe`** (ou em `Iniciar DISC2.bat`).

## Como Compilar do Código-Fonte

```bash
cargo build --release
```
O binário otimizado será gerado em `target/release/disc2.exe`.
