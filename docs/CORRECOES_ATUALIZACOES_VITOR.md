# Lista de Correções e Limpezas — Branch `Atualizacoes-VITOR`

> Arquivo exclusivo da branch `Atualizacoes-VITOR` para rastrear correções de bugs e limpeza de features durante os testes.

---

## Já concluído

- [x] **Remover feature Modo Turbo (16)**
  - Backend: removido o comando RPC `"activate_turbo_mode"` em `src/bridge/mod.rs`.
  - Frontend: removidos botão, estado `isTurbo`, prop `onActivateTurbo`, import `Zap` e rótulo `Turbo (65%)` em `SZweb/src/components/Header.tsx`, `SZweb/src/App.tsx` e `SZweb/src/services/backendBridge.ts`.

---

## Correções pendentes (Backend / Runtime)

- [ ] **0. Erro principal: parou de transmitir tela após a junção das branches (`0x0 • 0 FPS • 0 frames recebidos`)**
  - **Sintoma:** na `v2.11.0` a transmissão funcionava; após a junção das branches (`SZweb`), ao iniciar transmissão o card fica em `LIVE` com `0x0 • 0 FPS • 0.0 Mbps`, WebSocket conectado com `0 frames recebidos` e telemetria `Local 0 FPS`. As tentativas iniciais (#1, #2, #6) ainda não resolveram nos testes.
  - **A investigar:** fluxo completo `start_sharing` → `FastCapturer::capture_live_frame` → `encode_tx` → `Encoder` → `publish_video_frame` → WebSocket `/ws` → canvas do `SZweb` (incluindo se o processo fecha/crasha ou se algum gate bloqueia os quadros).

- [ ] **1. Corrigir `is_szdisc_in_foreground()` e prévia local com `autoGameEco`**
  - **Onde:** `src/capture.rs` (`is_szdisc_in_foreground`, `is_szdisc_window_match`, `is_browser_exe`), `src/stream_controller.rs` (`need_local_preview`).
  - **Problema:** `is_szdisc_in_foreground()` comparava `GetForegroundWindow()` com `GetCurrentProcessId()`, mas o `DISC2.exe` agora é headless e a janela visível pertence ao navegador (`SZweb`). Como `auto_game_eco` inicia como `true`, `need_local_preview` ficava sempre `false` e a prévia local (`tileId: 0`) nunca recebia frames.
  - **Status:** ajuste inicial aplicado, mas ainda pendente de validação real (não resolveu a transmissão nos testes).

- [ ] **2. Corrigir índice DXGI `1-based` vs `0-based` para monitores**
  - **Onde:** `src/capture.rs` (`monitor_enum_proc` e `DxgiDuplicator::init_for_region`).
  - **Problema:** `monitor_enum_proc` atribuía `index` começando em `1`, mas `EnumOutputs(output_idx)` é `0-based`, causando falha em todo frame e recriação contínua do device D3D11.
  - **Status:** ajuste inicial aplicado (`init_for_region` + `dxgi_retry_after`), mas ainda pendente de validação real nos testes.

- [ ] **3. Otimizar `get_source_thumbnails` para não estourar timeout de 2,5s**
  - **Onde:** `src/bridge/mod.rs` (`capture_thumbnails_json`), `src/capture.rs` (`capture_thumbnail_rgba`).
  - **Problema:** `capture_thumbnails_json()` captura e codifica todas as janelas e monitores em sequência numa única chamada RPC síncrona, podendo ultrapassar o `AbortSignal.timeout(2500)` do frontend. Além disso, `capture_thumbnail_rgba()` passa `need_rgba_preview = true` e aloca um buffer `rgb` extra que é descartado.
  - **Correção:** evitar alocação de `rgb` nas miniaturas, acelerar/limitar ou paralelizar a captura e evitar travamentos de `PrintWindow`.

- [ ] **4. Impedir captura da própria janela do `SZweb` e pop-outs**
  - **Onde:** `src/capture.rs` (`window_enum_proc`).
  - **Problema:** o filtro atual ignora apenas `SZdisc —` e `DISC2 —`, o que pode deixar passar janelas do `SZweb` em modo dev/pop-out e gerar efeito espelho.
  - **Correção:** filtrar de forma robusta janelas do `SZweb` e pop-outs na enumeração de janelas.

- [ ] **5. Tratar instância duplicada na porta `47839`**
  - **Onde:** `src/main.rs`, `src/bridge/mod.rs` (`BridgeServer::start`).
  - **Problema:** se a porta `47839` já estiver ocupada por outro `DISC2.exe`, a thread HTTP retorna silenciosamente, `main.rs` ainda abre outra janela apontando para a instância antiga e o segundo processo fica preso em loop infinito consumindo recursos.
  - **Correção:** detectar instância já ativa na porta `47839`, abrir/focar a janela existente e encerrar o processo duplicado.

- [ ] **6. Enviar frame imediato ao iniciar/trocar fonte com tela estática no DXGI**
  - **Onde:** `src/capture.rs` (`FastCapturer::force_next_frame`, `last_source_id`), `src/stream_controller.rs`, `src/bridge/mod.rs` (`WS_CONNECT_SEQ` / `ws_connect_seq`).
  - **Problema:** quando o DXGI retornava `Unchanged`, `capture_live_frame` retornava `None` por até `1s`, atrasando a exibição inicial ao trocar de fonte ou abrir um novo cliente WebSocket/pop-out.
  - **Status:** ajuste inicial aplicado, mas ainda pendente de validação real nos testes.

- [ ] **7. Sincronizar `my_peer_id` após onboarding/edição de perfil e limpar presença no `logout`**
  - **Onde:** `src/app.rs`, `src/bridge/mod.rs` (`complete_onboarding`, `save_profile`, `logout`), `src/network.rs` (`RoomHub`).
  - **Problema:** `my_peer_id` é derivado antes do onboarding (com `"Jogador"`) e não é atualizado ao criar/editar conta; no `logout`, o `server_lobby_sync_loop` continua anunciando presença no lobby do servidor.
  - **Correção:** atualizar identidade no `RoomHub` após onboarding/perfil e pausar anúncio de lobby quando `account_created == false`.

- [ ] **8. Corrigir ciclo de vida de memória na ativação assíncrona de áudio (`audio.rs`)**
  - **Onde:** `src/audio.rs` (`activate_process_specific_audio_client`).
  - **Problema:** `CompletionHandlerCom` é alocado na stack e espera até `2000ms` por `h_event`; se o callback COM disparar após timeout, acessa memória inválida na stack.
  - **Correção:** alocar o handler no heap com contagem de referência (`Arc`/`Box::into_raw` + `Release` real) e liberar `unk` em todos os caminhos de erro.

- [ ] **9. Pequenas limpezas no backend**
  - Remover campo morto `tile_order` de `AppCore` (`src/app.rs`).
  - Adicionar `Connection: close` na resposta `OPTIONS` em `src/bridge/mod.rs`.

---

## Auditoria de 16/11 — status dos itens acima

> Feita só lendo o código. Nada foi compilado nem executado.

| Item | Status | Observação |
|---|---|---|
| Turbo | ✅ Feito | Nenhuma ocorrência no backend nem no `SZweb/src`. |
| 0 | ❌ Aberto | O formato do WS (`[u32 tile][u16 w][u16 h][jpeg]`) e a URL batem entre Rust e TS. `Local 0 FPS` + `0x0` vêm do snapshot RPC: **o encoder nunca recebe frame**, então o problema está antes de `publish_video_frame`. Hipóteses abaixo. |
| 1 | 🟡 Parcial | O código está correto (`capture.rs:402-424`). O gate em `stream_controller.rs:290-298`, porém, para a captura quando você está sozinho na sala e a janela em foco não é o SZweb. |
| 2 | ✅ Feito (falta validar) | `saturating_sub(1)` + `init_for_region` por coordenadas + `dxgi_retry_after`. |
| 3 | ❌ Aberto | `mod.rs:916-933` continua sequencial. O frontend ainda usa timeout único de 2500ms. |
| 4 | 🟡 Parcial | `capture.rs:334-335` só filtra pelo prefixo do título. Usar `is_szdisc_window_match(title, exe)` e excluir `pid == GetCurrentProcessId()`. |
| 5 | ❌ Aberto | `mod.rs:725-728` retorna em silêncio. `AppCore::new()` roda **antes** do bind. |
| 6 | ✅ Feito (falta validar) | `force_next_frame` é chamado na troca de fonte e quando chega um cliente WS novo. |
| 7 | ❌ Aberto | `hub.my_peer_id` nunca é atualizado. O `logout` não limpa `viewed_server_id`. |
| 8 | ❌ Aberto | O handler continua na stack (`audio.rs:234-268`). O retorno de `WaitForSingleObject` não é checado. `unk` vaza nos caminhos de erro. |
| 9 | ❌ Aberto | `tile_order` ainda existe (`app.rs:30,111`). Ao remover, apagar também `stream_controller.rs:373`. Falta `Connection: close` em `mod.rs:821`. |

### Hipóteses para o item 0 (testar nesta ordem)
1. **Frontend em modo Mock:** um timeout de RPC (`backendBridge.ts:184`, 2500ms) ativa `mode='mock'`, e o mock de `start_sharing` marca `isSharing=true`. Isso dá `LIVE` sem transmissão real. Conferir o badge "Backend Rust offline".
2. **`dist` embutido desatualizado:** `build.rs:30-46` só roda `npm run build` se `SZweb/dist/index.html` não existir, e não observa `SZweb/src`. Testar com rebuild limpo (apagar `SZweb/dist`).
3. **Exe trocado pelo updater:** `updater.rs:164-236` baixa `main/DISC2.exe` mesmo se a consulta de versão falhar.
4. **Gate de foco/peers:** testar com `autoGameEco=false`.
5. **`active_source == None`:** se o `sourceId` não está na lista (`mod.rs:1282-1288`), o índice antigo é mantido e o laço roda sem capturar e sem log.
6. **Threads mortas:** `stream_controller.rs:157` não recria threads que já morreram (ver N1).
- Adicionar logs em `stream_controller.rs:295` (motivo do skip), `:304` (`source_opt`) e no `None` de `capture_live_frame`.

---

## Novos problemas encontrados (correções futuras)

### Alta
- [x] **N1. Threads de captura/encode mortas não são recriadas:** `stream_controller.rs` agora acompanha `capture_alive` e `encoder_alive` (com `AliveGuard` via `Drop`) e recria as threads se alguma tiver morrido.
- [x] **N2. Lock do `AppCore` na thread de tick:** `pick_profile_image` agora abre o diálogo e codifica o JPEG **fora** do lock do `AppCore`, e `custom_json` (com data-URLs de avatar/banner) é cacheado em `cached_custom_json` para não ser realocado a cada 200ms.
- [x] **N3. `build.rs` embute um `dist` velho:** adicionado `rerun-if-changed` para `SZweb/src`, `SZweb/index.html` e `SZweb/package.json`, além de verificação de timestamp (`newest_mtime`) para regerar `SZweb/dist` quando o código-fonte for mais novo.
- [x] **N4. Updater perigoso em builds de branch:** `CURRENT_VERSION` agora usa `env!("CARGO_PKG_VERSION")`, `download_and_apply_update_async` aborta se a consulta de versão falhar ou for inválida, e o updater fica desativado em builds debug ou quando `SZDISC_DISABLE_UPDATER` estiver definida.

### Média
- [ ] **N5.** `start_sharing` com `sourceId` desconhecido mantém o índice antigo (`mod.rs:1282-1288`). Retornar `Err` e guardar o `source.id` no lugar do índice (`stream_controller.rs:100`).
- [ ] **N6.** `ensure_dib_surface` → `release_gdi` (`capture.rs:800-803,1002`) também destrói o `DxgiDuplicator`. Separar `release_dxgi`.
- [ ] **N7.** `AcquireNextFrame(0)` (`capture.rs:659`) dá cerca de 1 FPS com tela estática. Usar timeout de 8 a 16ms ou reenviar o último frame DXGI.
- [ ] **N8.** No DXGI, movimento só do cursor é tratado como `Unchanged`, e o cursor não é desenhado (`capture.rs:670,1024`).
- [ ] **N9.** O WS não tem write timeout (`mod.rs:764,528-545`): um cliente travado prende a thread. Usar `set_write_timeout(2s)`.
- [ ] **N10.** O leitor WS trata cada `read` como um frame inteiro (`mod.rs:536`), então pode ler um close falso.
- [ ] **N11.** Um `tileId` inválido no WS faz o cliente receber **todos** os tiles (`mod.rs:811-814`).
- [ ] **N12.** Contador "frames recebidos" travado na tela (`videoStreamClient.ts:148`): a mutação não chama `emitStats`, e frames sem canvas não são contados.
- [ ] **N13.** Usar timeout por RPC no frontend (10s para `get_source_thumbnails` e `start_sharing`) e não tratar `TimeoutError` como queda da ponte.
- [ ] **N14.** O updater faz `exit(0)` sem esperar a porta 47839 ser liberada (`updater.rs:235`). Combina com o item 5.
- [ ] **N15.** Áudio: quando `GetNextPacketSize` falha, o cliente é recriado em loop sem backoff (`audio.rs:420-422`).

### Baixa
- [ ] **N16.** Mojibake (`Ã—`, `â€¢`) em `App.tsx:232,240`, que aparece na interface. Salvar em UTF-8.
- [ ] **N17.** Remover `SZweb/src/models/Header.tsx` (re-export morto).
- [ ] **N18.** Polling de 500ms sem trava `inFlight` (`App.tsx:93-95`).
- [ ] **N19.** `catch {}` vazio em `drainTile` (`videoStreamClient.ts:201`). O canvas também não é limpo ao parar.
- [ ] **N20.** `network.rs:1117`: `lock().unwrap()` pode dar panic. Usar `if let Ok`.
- [ ] **N21.** `Content-Length` sem limite (`mod.rs:832-838`).
- [ ] **N22.** `main.rs:62-73` abre qualquer processo que esteja na porta 5173 como se fosse a UI.
- [ ] **N23.** `capture.rs:909-932`: `.max(1920/1080)` e `.max(64)` geram recortes fora dos limites ou upscale. Janela com recorte negativo deve retornar `None`.
- [ ] **N24.** DXGI só usa o adapter padrão (`capture.rs:510-525`), então em notebook híbrido sempre cai no GDI.
- [ ] **N25.** Silêncio (chunks zerados) é descartado no áudio (`audio.rs:449-453`).
- [ ] **N26.** `release.yml` só roda na `main`. `build.rs` falha em silêncio sem `windres`. A versão continua `2.11.0`.

---

## Limpeza de features (a definir)

> Anotar aqui outras features da lista geral que forem selecionadas para remoção ou simplificação nesta branch.

- [ ] *(Nenhuma adicional selecionada ainda)*

---

## Finalização para testes (ao concluir as correções)

- [ ] Rodar `npm run lint` e `npm run build` em `SZweb/`.
- [ ] Rodar `cargo test` na raiz.
- [ ] Atualizar `DISC2.exe` (`cargo build --release` e copiar o binário final, se aplicável aos testes locais).
