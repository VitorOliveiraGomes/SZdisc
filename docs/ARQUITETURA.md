# Arquitetura Modular — ScreenZ

Estrutura organizada por responsabilidades no backend Rust (`src/`) e por seções funcionais no frontend (`ScreenZWeb/src/`).

## Backend Rust (`src/`)

- `src/main.rs` — Ponto de entrada, configuração de DPI awareness (`PerMonitorV2`) e inicialização do aplicativo desktop.
- `src/desktop.rs` — Runtime Tauri 2, instância única, comandos IPC `bridge_state`/`bridge_rpc` e ciclo de vida das janelas principal e de transmissão. Fechar uma janela de transmissão devolve a tela ao painel. Fechar a janela principal encerra a sessão.
- `tauri.conf.json` e `capabilities/desktop.json` — Build, conteúdo embutido, janelas e permissões das APIs desktop.
- `src/config/` — Configurações, perfil, temas e persistência:
  - `models.rs` — `UserCustomization`, `ServerProfile`, `PresenceStatus`, `ThemePreset`.
  - `storage.rs` — Leitura e gravação de `%APPDATA%/ScreenZ/account_v2.conf`, com importação do perfil legado.
  - `media.rs` — Seletor nativo de imagens, redimensionamento JPEG e `encode_base64`.
- `src/core/` — Núcleo de estado e pipeline de transmissão:
  - `state.rs` — `AppCore`, inicialização e `tick()` periódico (200ms).
  - `commands.rs` — Execução dos comandos RPC com deserialização via `serde_json`.
  - `stream.rs` — Pipeline zero-lag de 2 estágios (Captura -> Encoder JPEG com pool de buffers RGB).
- `src/capture/` — Captura de tela multiplataforma:
  - `windows/enumerate.rs` — Enumeração de monitores/janelas, filtro do próprio `ScreenZWeb`/pop-outs e detecção de foco.
  - `windows/dxgi.rs` — `DxgiDuplicator` multi-adaptador (`IDXGIFactory1`) com cache de último frame.
  - `windows/gdi.rs` — `GdiSurface`, captura de miniaturas sem alocação RGB extra e overlay de cursor.
  - `windows/scaler.rs` — Downscale bilinear 2×2 em inteiros e conversão BGRA -> RGB/RGBA.
  - `unix.rs` — Implementação `xcap` para Linux/macOS.
- `src/audio/` — Captura e reprodução de áudio:
  - `windows_capture.rs` — WASAPI Process Loopback (por PID) e loopback do sistema com handler COM em heap.
  - `windows_playback.rs` — Player em anel via `waveOut`.
  - `unix.rs` — Player `cpal` para Linux/macOS.
- `src/network/` — Rede P2P / Salas e Lobbie:
  - `protocol.rs` — Cabeçalhos (`HLO6`, `INF6`, `VID6`, `AUD6`, `STP6`, `BYE6`), portas determinísticas e I/O TCP interrompível.
  - `bore.rs` — Transporte via `bore.pub` isolado (controle JSON via `serde_json`, auto-mesh da sala e sync de lobby).
  - `hub.rs` — `RoomHub`, identidade sincronizada, presença e filas por conexão.
- `src/bridge/` — Ponte local HTTP + WebSocket para o `ScreenZWeb` (`127.0.0.1:47839`):
  - `dto.rs` — DTOs tipados (`serde::Serialize` / `Deserialize`) e geração do snapshot.
  - `http.rs` — Servidor HTTP, assets embutidos (`ScreenZWeb/dist`), validação de `Content-Length` e `/ws`.
  - `ws.rs` — WebSocket binário zero-backpressure (`[tile_id][w][h][jpeg]`) com parser RFC 6455.
  - `rpc.rs` — Roteador `/api/rpc` e geração paralela de miniaturas.
- `src/updater.rs` — Auto-atualização pelo `VERSION.txt` e `ScreenZ.exe` do repositório GitHub (`WinInet` / `curl`).

## Frontend React (`ScreenZWeb/src/`)

- `src/types/bridge.ts` — Contratos TypeScript alinhados aos DTOs do backend Rust.
- `src/services/` — Comunicação e estado de baixo nível:
  - `backendBridge.ts` — Comandos IPC no Tauri e cliente HTTP/RPC com timeouts por método no navegador.
  - `mockBridge.ts` — Simulação local quando o backend `:47839` está offline.
  - `videoStreamClient.ts` — Cliente WebSocket binário com drenagem por `tileId`.
  - `themeSync.ts` — Sincronização de variáveis CSS do tema.
- `src/hooks/useBridgeState.ts` — Hook de polling deduplicado e chamadas RPC.
- `src/sections/` — Interface separada por seções:
  - `onboarding/OnboardingScreen.tsx`
  - `popout/PopoutStage.tsx`
  - `navigation/SpacesNav.tsx` e `rooms/RoomsPanel.tsx`
  - `topbar/HeaderBar.tsx`
  - `stage/StreamStage.tsx` e `SessionBar.tsx`
  - `modals/` (`ServerModal.tsx`, `SharePickerDialog.tsx`, `BridgeInspectorDialog.tsx`, `settings/*`)
- `src/components/` — Componentes compartilhados (`ProfileCard.tsx`, `StreamTile.tsx`, `ui/*`).
