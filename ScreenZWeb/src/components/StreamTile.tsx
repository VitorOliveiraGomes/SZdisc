import { useEffect, useRef, useState } from 'react'
import {
  ArrowLeftRight,
  ExternalLink,
  Focus,
  LayoutGrid,
  Maximize2,
  Minimize2,
  Radio,
  Volume2,
  VolumeX,
} from 'lucide-react'

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
  onMoveTile?: () => void
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
  onMoveTile,
  onPopout,
  onToggleFullscreen,
}: StreamTileProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [isFullscreen, setIsFullscreen] = useState(false)

  useEffect(() => {
    videoStreamClient.registerCanvas(tileId, canvasRef.current)
    return () => videoStreamClient.registerCanvas(tileId, null)
  }, [tileId])

  useEffect(() => {
    const onChange = () => setIsFullscreen(document.fullscreenElement === containerRef.current)
    document.addEventListener('fullscreenchange', onChange)
    return () => document.removeEventListener('fullscreenchange', onChange)
  }, [])

  const handleFullscreen = async () => {
    try {
      if (onToggleFullscreen) {
        onToggleFullscreen()
      } else if (document.fullscreenElement) {
        await document.exitFullscreen()
      } else {
        await containerRef.current?.requestFullscreen()
      }
    } catch (error) {
      console.error('Falha ao alternar tela cheia', error)
    }
  }

  return (
    <article ref={containerRef} className="stream-tile" aria-label={`Transmissão de ${title}`}>
      <div className="stream-heading">
        <div className="flex min-w-0 items-center gap-2">
          <span className="status-dot is-online" />
          <span className="truncate text-xs font-semibold">{title}</span>
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          {onToggleFocus && (
            <Button
              size="icon"
              variant={isFocused ? 'secondary' : 'ghost'}
              className="size-7"
              title={isFocused ? 'Ver todas as telas' : 'Focar nesta tela'}
              aria-label={isFocused ? 'Ver todas as telas' : 'Focar nesta tela'}
              onClick={onToggleFocus}
            >
              {isFocused ? <LayoutGrid className="size-3.5" /> : <Focus className="size-3.5" />}
            </Button>
          )}
          {!isFocused && onMoveTile && (
            <Button
              size="icon"
              variant="ghost"
              className="size-7"
              title="Mover tela"
              aria-label="Mover tela"
              onClick={onMoveTile}
            >
              <ArrowLeftRight className="size-3.5" />
            </Button>
          )}
          {!isPopout && onPopout && (
            <Button
              size="icon"
              variant="ghost"
              className="size-7"
              title="Abrir em outro monitor"
              aria-label="Abrir em outro monitor"
              onClick={onPopout}
            >
              <ExternalLink className="size-3.5" />
            </Button>
          )}
          <Button
            size="icon"
            variant="ghost"
            className="size-7"
            title={isFullscreen ? 'Sair da tela cheia' : 'Tela cheia'}
            aria-label={isFullscreen ? 'Sair da tela cheia' : 'Tela cheia'}
            onClick={() => void handleFullscreen()}
          >
            {isFullscreen ? <Minimize2 className="size-3.5" /> : <Maximize2 className="size-3.5" />}
          </Button>
        </div>
      </div>
      <div className="stream-image" onDoubleClick={() => void handleFullscreen()}>
        <canvas
          ref={canvasRef}
          className="stream-canvas"
          width={1280}
          height={720}
          aria-label={`Vídeo de ${title}`}
        />
        <div className="stream-placeholder">
          <Radio className="size-4 text-primary" />
          Aguardando imagem
        </div>
      </div>
      <div className="stream-metrics">
        <span className="min-w-0 truncate" title={subtitle}>
          {wsFps > 0 ? `${wsFps} fps · ${wsKbps} kbps` : 'Ao vivo'}
        </span>
        {volumePercent !== undefined && muted !== undefined && onAudioChange && (
          <div className="flex shrink-0 items-center gap-2">
            <Button
              size="icon"
              variant="ghost"
              className="size-6"
              title={muted ? 'Ativar áudio' : 'Silenciar áudio'}
              aria-label={muted ? 'Ativar áudio' : 'Silenciar áudio'}
              onClick={() => onAudioChange(volumePercent, !muted)}
            >
              {muted ? (
                <VolumeX className="size-3.5 text-destructive" />
              ) : (
                <Volume2 className="size-3.5" />
              )}
            </Button>
            <Slider
              className="w-20"
              aria-label="Volume da transmissão"
              value={[volumePercent]}
              min={0}
              max={150}
              step={1}
              onValueChange={([value]) => onAudioChange(value ?? 100, muted)}
            />
            <span className="w-7 text-right tabular-nums">{volumePercent}%</span>
          </div>
        )}
      </div>
    </article>
  )
}
