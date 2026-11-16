import { useEffect, useRef } from 'react'
import { ExternalLink, Maximize2, Minimize2, Volume2, VolumeX } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Slider } from '@/components/ui/slider'
import { videoStreamClient } from '@/services/videoStreamClient'

interface StreamTileProps {
  tileId: number
  title: string
  subtitle: string
  isFocused: boolean
  isPopout?: boolean
  wsFps?: number
  wsKbps?: number
  volumePercent?: number
  muted?: boolean
  onAudioChange?: (volumePercent: number, muted: boolean) => void
  onToggleFocus?: () => void
  onPopout?: () => void
  onToggleFullscreen?: () => void
}

export function StreamTile({
  tileId,
  title,
  subtitle,
  isFocused,
  isPopout = false,
  wsFps = 0,
  wsKbps = 0,
  volumePercent,
  muted,
  onAudioChange,
  onToggleFocus,
  onPopout,
  onToggleFullscreen,
}: StreamTileProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    videoStreamClient.registerCanvas(tileId, canvasRef.current)
    return () => {
      videoStreamClient.registerCanvas(tileId, null)
    }
  }, [tileId])

  const handleFullscreen = () => {
    if (onToggleFullscreen) {
      onToggleFullscreen()
      return
    }
    if (!document.fullscreenElement) {
      void containerRef.current?.requestFullscreen?.()
    } else {
      void document.exitFullscreen?.()
    }
  }

  return (
    <div
      ref={containerRef}
      className="relative flex h-full w-full flex-col overflow-hidden rounded-xl border border-border bg-[#0a0b0e] shadow-lg"
    >
      <div
        className="relative flex flex-1 items-center justify-center overflow-hidden bg-black/80"
        onDoubleClick={handleFullscreen}
      >
        <canvas
          ref={canvasRef}
          className="max-h-full max-w-full object-contain"
          width={1280}
          height={720}
        />
        {wsFps === 0 && (
          <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-1 text-center text-muted-foreground">
            <span className="text-xs font-medium">Aguardando quadros binários no WebSocket...</span>
            <span className="text-[11px] opacity-75">
              tile_id={tileId} • ws://127.0.0.1:47839/ws
            </span>
          </div>
        )}
      </div>

      <div className="flex h-10 shrink-0 items-center justify-between gap-2 border-t border-border bg-card px-3">
        <div className="flex min-w-0 items-center gap-2">
          <Badge variant="live" className="px-1.5 py-0 text-[10px]">
            ● LIVE
          </Badge>
          <span className="truncate text-xs font-semibold text-foreground">{title}</span>
          <span className="hidden truncate text-[11px] text-muted-foreground sm:inline">
            {subtitle}
            {wsFps > 0 ? ` • WS ${wsFps} fps (${wsKbps} kbps)` : ''}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          {volumePercent !== undefined && muted !== undefined && onAudioChange && (
            <div className="flex items-center gap-1.5 pr-1">
              <Button
                size="icon"
                variant="ghost"
                className="size-7"
                onClick={() => onAudioChange(volumePercent, !muted)}
                title={muted ? 'Desmutar áudio' : 'Mutar áudio'}
              >
                {muted ? <VolumeX className="size-3.5 text-red-400" /> : <Volume2 className="size-3.5" />}
              </Button>
              <div className="w-20">
                <Slider
                  value={[volumePercent]}
                  min={0}
                  max={150}
                  step={1}
                  onValueChange={([v]) => onAudioChange(v ?? 100, muted)}
                />
              </div>
              <span className="w-8 text-right text-[10px] text-muted-foreground">{volumePercent}%</span>
            </div>
          )}

          {onToggleFocus && (
            <Button
              size="sm"
              variant={isFocused ? 'default' : 'secondary'}
              className="h-7 px-2 text-xs"
              onClick={onToggleFocus}
            >
              {isFocused ? 'Ver Juntas' : 'Focar'}
            </Button>
          )}

          {!isPopout && onPopout && (
            <Button
              size="sm"
              variant="secondary"
              className="h-7 px-2 text-xs"
              onClick={onPopout}
              title="Abrir em outra janela para arrastar para outro monitor"
            >
              <ExternalLink className="size-3.5" /> Outro Monitor
            </Button>
          )}

          <Button size="sm" variant="default" className="h-7 px-2 text-xs" onClick={handleFullscreen}>
            {document.fullscreenElement ? (
              <Minimize2 className="size-3.5" />
            ) : (
              <Maximize2 className="size-3.5" />
            )}
            Tela Cheia
          </Button>
        </div>
      </div>
    </div>
  )
}
