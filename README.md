# SZdisc — Transmissão de Tela Nativa em Rust (1080p 60fps + Áudio)

Aplicativo desktop **100% nativo em Rust** (`egui` + OpenGL), focado em transmissão de tela e jogos em **1080p 60fps** com baixo consumo de recursos, sistema de **Servidores e Salas** estilo Discord e zero necessidade de VPN/Radmin.

## Downloads (Todas as Plataformas)

| Sistema | Download |
|---|---|
| **Windows (64-bit)** | [SZdisc-Windows-x64.exe](https://github.com/VitorOliveiraGomes/SZdisc/releases/latest/download/SZdisc-Windows-x64.exe) *(ou [DISC2.exe direto da branch main](https://github.com/VitorOliveiraGomes/SZdisc/raw/main/DISC2.exe))* |
| **Linux (x86_64)** | [SZdisc-Linux-x64](https://github.com/VitorOliveiraGomes/SZdisc/releases/latest/download/SZdisc-Linux-x64) |
| **macOS (Apple Silicon M1/M2/M3/M4)** | [SZdisc-macOS-arm64](https://github.com/VitorOliveiraGomes/SZdisc/releases/latest/download/SZdisc-macOS-arm64) |
| **macOS (Intel x64)** | [SZdisc-macOS-x64](https://github.com/VitorOliveiraGomes/SZdisc/releases/latest/download/SZdisc-macOS-x64) |

> **Linux / macOS**: após baixar, dê permissão de execução com `chmod +x SZdisc-*` e execute `./SZdisc-*`.

## Destaques

- **Captura acelerada por GPU no Windows (DXGI Desktop Duplication)** com fallback GDI automático, `DWMWA_EXTENDED_FRAME_BOUNDS` e skip de tela parada.
- **Multiplataforma**: suporte a **Windows**, **Linux** (`xcap` + `cpal`) e **macOS** (`xcap` + `cpal`) compilados automaticamente via GitHub Actions.
- **Servidores e Salas isoladas pela Internet**: crie servidores, compartilhe o código de convite e entre/saia de salas sem precisar de Radmin ou VPN.
- **Qualidade ajustável ao vivo**: `1080p`, `900p`, `720p`, `480p`, `360p` e `60 / 30 / 15 FPS`, com modo tela cheia e destaque para outros monitores.

## Como Compilar do Código-Fonte

```bash
cargo build --release
```
