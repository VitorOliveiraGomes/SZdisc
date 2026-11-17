import { Activity, SlidersHorizontal } from 'lucide-react'

import type {
  AppSnapshotDto,
  BridgeConnectionMode,
  ResolutionPreset,
  WsStreamStats,
} from '@/types/bridge'

interface SessionBarProps {
  snapshot: AppSnapshotDto
  mode: BridgeConnectionMode
  wsStats: WsStreamStats
  onUpdateStream: (patch: Record<string, unknown>) => void
  onOpenInspector: () => void
}

export function SessionBar({
  snapshot,
  mode,
  wsStats,
  onUpdateStream,
  onOpenInspector,
}: SessionBarProps) {
  const { stream, telemetry } = snapshot
  const online = mode === 'rust' && wsStats.connected
  const connectionLabel =
    mode === 'mock'
      ? 'Prévia local'
      : mode === 'offline'
        ? 'Reconectando'
        : online
          ? 'Conexão pronta'
          : 'Conectando vídeo'
  const qualityOptions = [90, 75, 65, 48]
  if (!qualityOptions.includes(stream.jpegQuality)) qualityOptions.push(stream.jpegQuality)

  return (
    <footer className="studio-footer">
      <button
        type="button"
        className="connection-status"
        onClick={onOpenInspector}
        title="Abrir diagnóstico da conexão"
      >
        <span className={`status-dot ${online ? 'is-online' : ''}`} />
        {connectionLabel}
        <Activity className="size-3 opacity-60" />
      </button>
      <div className="quality-controls" aria-label="Qualidade da transmissão">
        <SlidersHorizontal className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <label className="quality-field">
          <span>Resolução</span>
          <select
            aria-label="Resolução"
            value={stream.resolution}
            onChange={(event) =>
              onUpdateStream({ resolution: event.target.value as ResolutionPreset })
            }
          >
            {(['1080p', '900p', '720p', '480p', '360p'] as const).map((resolution) => (
              <option key={resolution} value={resolution}>
                {resolution}
              </option>
            ))}
          </select>
        </label>
        <label className="quality-field">
          <span>Quadros</span>
          <select
            aria-label="FPS"
            value={stream.fps}
            onChange={(event) => onUpdateStream({ fps: Number(event.target.value) })}
          >
            {[60, 30, 15].map((fps) => (
              <option key={fps} value={fps}>
                {fps} fps
              </option>
            ))}
          </select>
        </label>
        <label className="quality-field">
          <span>Imagem</span>
          <select
            aria-label="Qualidade da imagem"
            value={stream.jpegQuality}
            onChange={(event) => onUpdateStream({ jpegQuality: Number(event.target.value) })}
          >
            {qualityOptions
              .sort((a, b) => b - a)
              .map((quality) => (
                <option key={quality} value={quality}>
                  {quality}%
                </option>
              ))}
          </select>
        </label>
      </div>
      <span
        className="whitespace-nowrap text-[10px] text-muted-foreground"
        title={`ScreenZ ${snapshot.version} · memória utilizada`}
      >
        v{snapshot.version}
        <span className="mx-2 opacity-40">/</span>
        {telemetry.ramMb.toFixed(0)} MB
      </span>
    </footer>
  )
}
