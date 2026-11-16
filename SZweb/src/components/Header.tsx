import { Activity, Monitor, Palette, Radio, Zap } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { AppSnapshotDto, ResolutionPreset, WsStreamStats } from '@/models/types'
import type { BridgeConnectionMode } from '@/services/backendBridge'

interface HeaderProps {
  snapshot: AppSnapshotDto
  mode: BridgeConnectionMode
  wsStats: WsStreamStats
  focusedTileId: number | null
  onUnfocus: () => void
  onUpdateStream: (patch: Record<string, unknown>) => void
  onActivateTurbo: () => void
  onOpenSettings: () => void
  onOpenInspector: () => void
}

export function Header({
  snapshot,
  mode,
  wsStats,
  focusedTileId,
  onUnfocus,
  onUpdateStream,
  onActivateTurbo,
  onOpenSettings,
  onOpenInspector,
}: HeaderProps) {
  const { stream, telemetry, room } = snapshot
  const isTurbo =
    stream.resolution === '720p' &&
    stream.fps === 60 &&
    stream.jpegQuality === 65 &&
    stream.autoGameEco

  return (
    <header className="flex h-12 shrink-0 items-center justify-between gap-2 border-b border-border bg-card px-3">
      <div className="flex items-center gap-2 overflow-x-auto">
        {(stream.isSharing || room.peers.length > 0) && (
          <Badge variant="live" className="gap-1">
            <Radio className="size-3" /> AO VIVO
          </Badge>
        )}

        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span className="hidden sm:inline font-medium text-foreground/80">Qualidade:</span>
          <select
            aria-label="Resolução"
            value={stream.resolution}
            onChange={(e) => onUpdateStream({ resolution: e.target.value as ResolutionPreset })}
            className="h-7 rounded-md border border-input bg-secondary px-2 text-xs text-foreground outline-none focus:border-ring"
          >
            <option value="1080p">1080p</option>
            <option value="900p">900p</option>
            <option value="720p">720p</option>
            <option value="480p">480p</option>
            <option value="360p">360p</option>
          </select>

          <select
            aria-label="FPS"
            value={stream.fps}
            onChange={(e) => onUpdateStream({ fps: Number(e.target.value) })}
            className="h-7 rounded-md border border-input bg-secondary px-2 text-xs text-foreground outline-none focus:border-ring"
          >
            <option value={60}>60 FPS</option>
            <option value={30}>30 FPS</option>
            <option value={15}>15 FPS</option>
          </select>

          <select
            aria-label="Qualidade JPEG"
            value={stream.jpegQuality}
            onChange={(e) => onUpdateStream({ jpegQuality: Number(e.target.value) })}
            className="h-7 rounded-md border border-input bg-secondary px-2 text-xs text-foreground outline-none focus:border-ring"
          >
            <option value={90}>Ultra (90%)</option>
            <option value={75}>Alta (75%)</option>
            <option value={65}>Turbo (65%)</option>
            <option value={48}>Leve (48%)</option>
          </select>
        </div>

        <Button
          size="sm"
          variant={isTurbo ? 'success' : 'secondary'}
          className="h-7 px-2.5 text-xs"
          onClick={onActivateTurbo}
        >
          <Zap className="size-3.5" /> Modo Turbo
        </Button>

        {focusedTileId !== null && (
          <Button size="sm" variant="outline" className="h-7 px-2.5 text-xs" onClick={onUnfocus}>
            <Monitor className="size-3.5" /> Ver Telas Juntas
          </Button>
        )}
      </div>

      <div className="flex items-center gap-2 shrink-0">
        <button
          type="button"
          onClick={onOpenInspector}
          className="flex items-center gap-1.5 rounded-md border border-border bg-secondary/80 px-2.5 py-1 text-xs transition-colors hover:bg-accent cursor-pointer"
          title="Abrir Console de Teste da Ponte Rust ↔ SZweb"
        >
          <span
            className={`size-2 rounded-full ${mode === 'rust' ? 'bg-emerald-400' : 'bg-amber-400'}`}
          />
          <span className="font-medium">
            {mode === 'rust' ? `Rust :${snapshot.bridgePort}` : 'Mock Local'}
          </span>
          <span className="text-muted-foreground">|</span>
          <span className={wsStats.connected ? 'text-emerald-400' : 'text-zinc-400'}>
            WS {wsStats.connected ? 'ON' : 'OFF'}
          </span>
        </button>

        <Badge variant="success" className="hidden md:inline-flex">
          ⚡ v{snapshot.version} • RAM {telemetry.ramMb.toFixed(0)}MB
        </Badge>

        <Button size="sm" variant="outline" className="h-7 px-2.5 text-xs" onClick={onOpenInspector}>
          <Activity className="size-3.5" /> Testar Ponte
        </Button>

        <Button size="sm" variant="secondary" className="h-7 px-2.5 text-xs" onClick={onOpenSettings}>
          <Palette className="size-3.5" /> Conta & Tema
        </Button>
      </div>
    </header>
  )
}
