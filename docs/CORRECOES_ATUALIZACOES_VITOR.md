# Lista de Correções e Limpezas — Branch `Atualizacoes-VITOR`

> Arquivo exclusivo da branch `Atualizacoes-VITOR` para rastrear correções de bugs e limpeza de features durante os testes.

---

## Já concluído

- [x] **Remover feature Modo Turbo (16)**
  - Backend: removido o comando RPC `"activate_turbo_mode"` em `src/bridge/mod.rs`.
  - Frontend: removidos botão, estado `isTurbo`, prop `onActivateTurbo`, import `Zap` e rótulo `Turbo (65%)` em `ScreenZWeb/src/components/Header.tsx`, `ScreenZWeb/src/App.tsx` e `ScreenZWeb/src/services/backendBridge.ts`.

---

## Correções pendentes (Backend / Runtime)

- [x] **0. Erro principal (`0x0 • 0 FPS • 0 frames recebidos`) — correções aplicadas na reformulação**
  - **Onde:** `app.manifest`, `src/main.rs`, `src/core/stream.rs`, `src/capture/windows/{dxgi.rs,mod.rs,gdi.rs}`, `ScreenZWeb/src/services/backendBridge.ts`.
  - **Correções aplicadas:** ativado `PerMonitorV2` DPI awareness, separação `release_dxgi`/`release_gdi`, `AcquireNextFrame(8)` com cache de último frame estático, grace period inicial de 1.5s para entrega imediata de prévia WS mesmo com `autoGameEco`, remoção de fallback para Mock em timeouts transitórios e trava de instância única na porta `47839`. *(Aguarda validação interativa em runtime desktop.)*

- [x] **1. Corrigir `is_screenz_in_foreground()` e prévia local com `autoGameEco`**
  - **Onde:** `src/capture/windows/enumerate.rs`, `src/core/stream.rs`.

- [x] **2. Corrigir índice DXGI `1-based` vs `0-based` para monitores**
  - **Onde:** `src/capture/windows/dxgi.rs` (`init_for_region` multi-adaptador).

- [x] **3. Otimizar `get_source_thumbnails` para não estourar timeout de 2,5s**
  - **Onde:** `src/bridge/rpc.rs` (`capture_thumbnails_json` em paralelo com até 4 workers), `src/capture/windows/scaler.rs` (`capture_rgb = false` sem alocar buffer RGB descartado), `ScreenZWeb/src/services/backendBridge.ts` (timeout de 10s).

- [x] **4. Impedir captura da própria janela do `ScreenZWeb` e pop-outs**
  - **Onde:** `src/capture/windows/enumerate.rs` (`window_enum_proc` exclui `GetCurrentProcessId()`, `ScreenZ Pop-out` e `is_screenz_window_match`), `ScreenZWeb/src/sections/popout/PopoutStage.tsx`.

- [x] **5. Tratar instância duplicada na porta `47839`**
  - **Onde:** `src/main.rs` (`is_screenz_bridge_running`, `--wait-replace`), `src/bridge/mod.rs` (`BridgeServer::start` retorna `Result`).

- [x] **6. Enviar frame imediato ao iniciar/trocar fonte com tela estática no DXGI**
  - **Onde:** `src/core/stream.rs`, `src/capture/windows/{mod.rs,dxgi.rs}`.

- [x] **7. Sincronizar `my_peer_id` após onboarding/edição de perfil e limpar presença no `logout`**
  - **Onde:** `src/core/state.rs` (`sync_identity_to_hub`), `src/core/commands.rs`, `src/network/hub.rs` (`update_identity`, `set_account_active`), `src/network/bore.rs`.

- [x] **8. Corrigir ciclo de vida de memória na ativação assíncrona de áudio (`audio.rs`)**
  - **Onde:** `src/audio/windows_capture.rs` (`CompletionHandlerCom` alocado no heap com refcounting + `Drop` fechando `h_event` e checagem de `WAIT_OBJECT_0` e liberação de `unk`).

- [x] **9. Pequenas limpezas no backend**
  - Removido campo morto `tile_order` de `AppCore` (`src/core/state.rs`).
  - Adicionado `Connection: close` na resposta `OPTIONS` em `src/bridge/http.rs`.

---

## Auditoria de 16/11 — status dos itens acima

> Feita só lendo o código. Nada foi compilado nem executado.

| Item | Status | Observação |
|---|---|---|
| Turbo | ✅ Feito | Nenhuma ocorrência no backend nem no `ScreenZWeb/src`. |
| 0 | ❌ Aberto | O formato do WS (`[u32 tile][u16 w][u16 h][jpeg]`) e a URL batem entre Rust e TS. `Local 0 FPS` + `0x0` vêm do snapshot RPC: **o encoder nunca recebe frame**, então o problema está antes de `publish_video_frame`. Hipóteses abaixo. |
| 1 | 🟡 Parcial | O código está correto (`capture.rs:402-424`). O gate em `stream_controller.rs:290-298`, porém, para a captura quando você está sozinho na sala e a janela em foco não é o ScreenZWeb. |
| 2 | ✅ Feito (falta validar) | `saturating_sub(1)` + `init_for_region` por coordenadas + `dxgi_retry_after`. |
| 3 | ❌ Aberto | `mod.rs:916-933` continua sequencial. O frontend ainda usa timeout único de 2500ms. |
| 4 | 🟡 Parcial | `capture.rs:334-335` só filtra pelo prefixo do título. Usar `is_screenz_window_match(title, exe)` e excluir `pid == GetCurrentProcessId()`. |
| 5 | ❌ Aberto | `mod.rs:725-728` retorna em silêncio. `AppCore::new()` roda **antes** do bind. |
| 6 | ✅ Feito (falta validar) | `force_next_frame` é chamado na troca de fonte e quando chega um cliente WS novo. |
| 7 | ❌ Aberto | `hub.my_peer_id` nunca é atualizado. O `logout` não limpa `viewed_server_id`. |
| 8 | ❌ Aberto | O handler continua na stack (`audio.rs:234-268`). O retorno de `WaitForSingleObject` não é checado. `unk` vaza nos caminhos de erro. |
| 9 | ❌ Aberto | `tile_order` ainda existe (`app.rs:30,111`). Ao remover, apagar também `stream_controller.rs:373`. Falta `Connection: close` em `mod.rs:821`. |

### Hipóteses para o item 0 (testar nesta ordem)
1. **Frontend em modo Mock:** um timeout de RPC (`backendBridge.ts:184`, 2500ms) ativa `mode='mock'`, e o mock de `start_sharing` marca `isSharing=true`. Isso dá `LIVE` sem transmissão real. Conferir o badge "Backend Rust offline".
2. **`dist` embutido desatualizado:** `build.rs:30-46` só roda `npm run build` se `ScreenZWeb/dist/index.html` não existir, e não observa `ScreenZWeb/src`. Testar com rebuild limpo (apagar `ScreenZWeb/dist`).
3. **Exe trocado pelo updater:** `updater.rs:164-236` baixa `main/ScreenZ.exe` mesmo se a consulta de versão falhar.
4. **Gate de foco/peers:** testar com `autoGameEco=false`.
5. **`active_source == None`:** se o `sourceId` não está na lista (`mod.rs:1282-1288`), o índice antigo é mantido e o laço roda sem capturar e sem log.
6. **Threads mortas:** `stream_controller.rs:157` não recria threads que já morreram (ver N1).
- Adicionar logs em `stream_controller.rs:295` (motivo do skip), `:304` (`source_opt`) e no `None` de `capture_live_frame`.

---

## Novos problemas encontrados (correções futuras)

### Alta
- [x] **N1. Threads de captura/encode mortas não são recriadas:** `stream_controller.rs` agora acompanha `capture_alive` e `encoder_alive` (com `AliveGuard` via `Drop`) e recria as threads se alguma tiver morrido.
- [x] **N2. Lock do `AppCore` na thread de tick:** `pick_profile_image` agora abre o diálogo e codifica o JPEG **fora** do lock do `AppCore`, e `custom_json` (com data-URLs de avatar/banner) é cacheado em `cached_custom_json` para não ser realocado a cada 200ms.
- [x] **N3. `build.rs` embute um `dist` velho:** adicionado `rerun-if-changed` para `ScreenZWeb/src`, `ScreenZWeb/index.html` e `ScreenZWeb/package.json`, além de verificação de timestamp (`newest_mtime`) para regerar `ScreenZWeb/dist` quando o código-fonte for mais novo.
- [x] **N4. Updater perigoso em builds de branch:** `CURRENT_VERSION` agora usa `env!("CARGO_PKG_VERSION")`, `download_and_apply_update_async` aborta se a consulta de versão falhar ou for inválida, e o updater fica desativado em builds debug ou quando `SCREENZ_DISABLE_UPDATER` estiver definida.

### Média
- [x] **N5.** `start_sharing` agora guarda `active_source_id: String` e retorna `Err` se `sourceId` não for encontrado (`src/core/commands.rs`, `src/core/state.rs`).
- [x] **N6.** `GdiSurface::release` separado de `DxgiDuplicator::release_all` (`src/capture/windows/gdi.rs`).
- [x] **N7.** `AcquireNextFrame(8)` + cache `last_bgra` em `DxgiDuplicator` para tela estática (`src/capture/windows/dxgi.rs`).
- [x] **N8.** Rastreamento de movimento do cursor + overlay em buffer BGRA (`src/capture/windows/{gdi.rs,mod.rs}`).
- [x] **N9.** `set_write_timeout(2s)` no WebSocket (`src/bridge/ws.rs`).
- [x] **N10.** Parser de frame RFC 6455 completo no leitor WebSocket (`src/bridge/ws.rs`).
- [x] **N11.** `tileId` inválido no `/ws` rejeitado com `400 Bad Request` (`src/bridge/http.rs`).
- [x] **N12.** `totalFrames` contabilizado e emitido corretamente (`ScreenZWeb/src/services/videoStreamClient.ts`).
- [x] **N13.** Timeout por RPC no frontend (`10s` para thumbnails/start_sharing) sem cair para Mock em `TimeoutError` (`ScreenZWeb/src/services/backendBridge.ts`).
- [x] **N14.** Updater passa `--wait-replace` e novo processo aguarda liberação da porta `47839` (`src/updater.rs`, `src/main.rs`).
- [x] **N15.** Backoff exponencial limitado (`300ms` a `1500ms`) em falhas WASAPI (`src/audio/windows_capture.rs`).

### Baixa
- [x] **N16.** Textos em UTF-8 limpo na UI (`ScreenZWeb/src/sections/`).
- [x] **N17.** Removidos `ScreenZWeb/src/models/Header.tsx`, `App.css` e assets de template Vite.
- [x] **N18.** Polling de 500ms protegido por `inFlightRef` (`ScreenZWeb/src/hooks/useBridgeState.ts`).
- [x] **N19.** Tratamento de erro no `drainTile` + limpeza de canvas ao parar/desmontar (`ScreenZWeb/src/services/videoStreamClient.ts`).
- [x] **N20.** Removidos `.lock().unwrap()` em `src/network/hub.rs`.
- [x] **N21.** Limite `MAX_HTTP_BODY_BYTES` (`2 MB`, `413 Payload Too Large`) em `src/bridge/http.rs`.
- [x] **N22.** Validação HTTP na porta `5173` restrita a debug (`src/main.rs`).
- [x] **N23.** Recorte de janelas sem `.max(1920)`/`.max(1080)` ou `.max(64)` (`src/capture/windows/{mod.rs,scaler.rs}`).
- [x] **N24.** Enumeração multi-adaptador DXGI via `CreateDXGIFactory1` (`src/capture/windows/dxgi.rs`).
- [x] **N25.** Cauda curta de silêncio (`3` pacotes) antes de pausar envio de áudio (`src/audio/windows_capture.rs`).
- [ ] **N26.** `release.yml` só roda na `main`. `build.rs` falha em silêncio sem `windres`. A versão continua `2.11.0`.

---

## Limpeza de features (a definir)

> Anotar aqui outras features da lista geral que forem selecionadas para remoção ou simplificação nesta branch.

- [ ] *(Nenhuma adicional selecionada ainda)*

---

## Finalização para testes (ao concluir as correções)

- [ ] Rodar `npm run lint` e `npm run build` em `ScreenZWeb/`.
- [ ] Rodar `cargo test` na raiz.
- [ ] Atualizar `ScreenZ.exe` (`cargo build --release` e copiar o binário final, se aplicável aos testes locais).
