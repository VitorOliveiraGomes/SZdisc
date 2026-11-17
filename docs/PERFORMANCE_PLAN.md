# Plano de Otimização de Performance — ScreenZ

Auditoria do pipeline de captura → codificação → rede → WebSocket → canvas e do
loop de estado (snapshot JSON + polling do ScreenZWeb).

## Gargalos encontrados

| # | Onde | Problema | Custo estimado |
|---|------|----------|----------------|
| 1 | `capture.rs` `capture_live_frame` | `vec![0u8; w*h*3]` novo a cada frame (1080p ≈ 6,2 MB zerados × 60 fps ≈ 370 MB/s de alocação + page faults) | Alto |
| 2 | `stream_controller.rs` | Quando o encoder está ocupado (`try_send` cheio) o buffer RGB do frame descartado é liberado em vez de reaproveitado | Alto (soma com #1) |
| 3 | `bridge/mod.rs` `publish_video_frame` | Pacote copiado inteiro (`to_vec`) para **cada** cliente WebSocket | Médio (N cópias de ~30–200 KB por frame) |
| 4 | `bridge/mod.rs` `write_ws_binary_frame` | Cabeçalho e payload em duas `write_all` com `TCP_NODELAY` → 2 syscalls/segmentos por frame | Baixo/Médio |
| 5 | `bridge/mod.rs` `json_str` | Escape char-a-char de avatar/banner em data URL (dezenas de KB) a cada snapshot (200 ms) | Médio |
| 6 | `bridge/mod.rs` snapshot | `remote_streams.lock()` adquirido uma vez **por peer** dentro do lock de `peers` | Baixo (contenção com threads de rede) |
| 7 | `bridge/mod.rs` thumbnails | Conversão RGBA→RGB manual antes do JPEG; o encoder aceita RGBA direto | Baixo |
| 8 | `stream_controller.rs` | `is_screenz_in_foreground()` (syscall) chamado a cada frame mesmo quando o resultado não importa | Baixo |
| 9 | `network.rs` writer | `flush()` em `TcpStream` após cada vídeo (no-op) | Trivial |
| 10 | `videoStreamClient.ts` | `buffer.slice(8)` copia o JPEG; `getContext('2d')` a cada frame; um frame corrompido encerra o loop de drenagem | Médio no front |
| 11 | `App.tsx` / `backendBridge.ts` | Poll de 500 ms sempre gera novo objeto → re-render completo do App e reescrita de 18 variáveis CSS (recalc de estilo do documento inteiro) mesmo sem mudança | Médio no front |
| 12 | `videoStreamClient.ts` stats | Emite stats a cada 1 s mesmo sem alteração → re-render do App | Baixo |

## Correções

1. **Reciclagem de buffers RGB** (#1, #2): `FastCapturer` guarda um buffer reserva;
   o encoder devolve o `Vec<u8>` por um canal de retorno, e o frame recusado por
   `try_send` também volta ao pool. Zero alocação no estado estacionário.
2. **Broadcast WebSocket sem cópias** (#3, #4): o pacote (cabeçalho WS + cabeçalho
   do tile + JPEG) é montado uma única vez em `Arc<Vec<u8>>` e compartilhado entre
   clientes; cada cliente faz uma única `write_all`.
3. **`json_str` com caminho rápido** (#5): varre bytes e faz `push_str` direto
   quando não há caracteres a escapar (caso de todo base64).
4. **Snapshot com lock único** (#6): `remote_streams` travado uma vez fora do loop.
5. **Thumbnails RGBA direto** (#7): `ColorType::Rgba` no encoder.
6. **Curto-circuito do foreground** (#8) e remoção do `flush` no-op (#9).
7. **Canvas** (#10): view `Uint8Array` sem cópia, contexto 2D cacheado
   (`alpha:false`, `desynchronized:true`), erro por frame não interrompe o loop.
8. **Polling deduplicado** (#11): `fetchState` compara o texto bruto e devolve o
   mesmo objeto se nada mudou (React descarta o `setState`); CSS vars só são
   reaplicadas quando a chave do tema muda.
9. **Stats** (#12): só emite quando os valores mudam.

## Verificação

- `cargo build --release`, `cargo clippy`, `cargo test`
- `npm run build` e `npm run lint` em `ScreenZWeb/`
