import { useState } from 'react'
import { Activity, RefreshCw, Send, Wifi, WifiOff } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import type { AppSnapshotDto, RpcLogEntry, WsStreamStats } from '@/models/types'
import type { BridgeConnectionMode } from '@/services/backendBridge'

interface BridgeInspectorDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  mode: BridgeConnectionMode
  snapshot: AppSnapshotDto
  wsStats: WsStreamStats
  logs: RpcLogEntry[]
  onRefreshState: () => void
  onSendToast: (msg: string) => void
  onCheckUpdates: () => void
}

export function BridgeInspectorDialog({
  open,
  onOpenChange,
  mode,
  snapshot,
  wsStats,
  logs,
  onRefreshState,
  onSendToast,
  onCheckUpdates,
}: BridgeInspectorDialogProps) {
  const [toastMsg, setToastMsg] = useState('Ping de teste vindo do SZweb (shadcn/ui)!')

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Activity className="size-5 text-primary" />
            Console de Teste da Ponte (Rust ↔ SZweb)
          </DialogTitle>
          <DialogDescription>
            Valida comunicação HTTP RPC (<code>127.0.0.1:47839/api/rpc</code>) e streaming binário WebSocket (<code>/ws</code>).
          </DialogDescription>
        </DialogHeader>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <div className="rounded-lg border border-border bg-secondary/60 p-3">
            <div className="text-xs text-muted-foreground">Status RPC</div>
            <div className="mt-1 flex items-center gap-2">
              {mode === 'rust' ? (
                <Badge variant="success">
                  <Wifi className="size-3" /> Conectado ao Rust
                </Badge>
              ) : (
                <Badge variant="outline" className="border-amber-500/40 text-amber-300">
                  <WifiOff className="size-3" /> Modo Mock Offline
                </Badge>
              )}
            </div>
          </div>

          <div className="rounded-lg border border-border bg-secondary/60 p-3">
            <div className="text-xs text-muted-foreground">WebSocket Binário (60fps)</div>
            <div className="mt-1 flex items-center gap-2">
              <Badge variant={wsStats.connected ? 'success' : 'secondary'}>
                {wsStats.connected ? 'Conectado' : 'Desconectado'}
              </Badge>
              <span className="text-xs text-muted-foreground">
                {wsStats.totalFrames} frames recebidos
              </span>
            </div>
          </div>

          <div className="rounded-lg border border-border bg-secondary/60 p-3">
            <div className="text-xs text-muted-foreground">Telemetria Rust</div>
            <div className="mt-1 text-xs font-medium">
              RAM {snapshot.telemetry.ramMb.toFixed(1)} MB • Local {snapshot.telemetry.localFps} FPS •{' '}
              {snapshot.telemetry.wsClients} clientes WS
            </div>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Input
            value={toastMsg}
            onChange={(e) => setToastMsg(e.target.value)}
            placeholder="Mensagem de toast para enviar ao Rust..."
            className="flex-1 min-w-52"
          />
          <Button size="sm" onClick={() => onSendToast(toastMsg)}>
            <Send className="size-3.5" /> Enviar Toast ao Rust
          </Button>
          <Button size="sm" variant="secondary" onClick={onRefreshState}>
            <RefreshCw className="size-3.5" /> Sync Estado
          </Button>
          <Button size="sm" variant="outline" onClick={onCheckUpdates}>
            Updater ({snapshot.updateStatus})
          </Button>
        </div>

        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          <div>
            <div className="mb-1.5 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              Histórico de Chamadas RPC
            </div>
            <ScrollArea className="h-56 rounded-lg border border-border bg-secondary/40 p-2">
              {logs.length === 0 ? (
                <div className="py-10 text-center text-xs text-muted-foreground">
                  Nenhuma chamada RPC disparada ainda nesta sessão.
                </div>
              ) : (
                <div className="flex flex-col gap-1.5">
                  {logs.map((entry) => (
                    <div
                      key={entry.id}
                      className="flex items-center justify-between gap-2 rounded-md border border-border/60 bg-card/70 px-2.5 py-1.5 text-xs"
                    >
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5">
                          <span
                            className={`size-1.5 rounded-full ${entry.ok ? 'bg-emerald-400' : 'bg-red-400'}`}
                          />
                          <code className="font-mono font-semibold text-foreground">{entry.method}</code>
                          <Badge variant="outline" className="px-1 py-0 text-[10px]">
                            {entry.mode}
                          </Badge>
                        </div>
                        <div className="truncate text-[11px] text-muted-foreground">{entry.detail}</div>
                      </div>
                      <div className="shrink-0 text-right text-[10px] text-muted-foreground">
                        <div>{entry.latencyMs}ms</div>
                        <div>{entry.timestamp}</div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </ScrollArea>
          </div>

          <div>
            <div className="mb-1.5 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              Snapshot JSON Atual
            </div>
            <ScrollArea className="h-56 rounded-lg border border-border bg-secondary/40 p-2">
              <pre className="text-[11px] leading-relaxed text-emerald-300/90">
                {JSON.stringify(
                  {
                    room: snapshot.room,
                    stream: snapshot.stream,
                    telemetry: snapshot.telemetry,
                    popoutTiles: snapshot.popoutTiles,
                  },
                  null,
                  2,
                )}
              </pre>
            </ScrollArea>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
