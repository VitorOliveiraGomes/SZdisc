import {
  ArrowDownToLine,
  ArrowUpRight,
  KeyRound,
  MonitorUp,
  Plus,
  Radio,
  Volume2,
  VolumeX,
} from 'lucide-react'
import { isTauri } from '@tauri-apps/api/core'

import { StreamTile } from '@/components/StreamTile'
import { Button } from '@/components/ui/button'
import type { AppSnapshotDto, WsStreamStats } from '@/types/bridge'

interface StreamStageProps {
  snapshot: AppSnapshotDto
  wsStats: WsStreamStats
  tileOrder: number[]
  focusedTileId: number | null
  onSetTileOrder: (order: number[]) => void
  onSetFocusedTileId: (tileId: number | null) => void
  onCallRpc: (method: string, params?: Record<string, unknown>) => Promise<void>
  onOpenSharePicker: () => void
  onOpenCreateSpace: () => void
  onOpenJoinSpace: () => void
}

export function StreamStage({
  snapshot,
  wsStats,
  tileOrder,
  focusedTileId,
  onSetTileOrder,
  onSetFocusedTileId,
  onCallRpc,
  onOpenSharePicker,
  onOpenCreateSpace,
  onOpenJoinSpace,
}: StreamStageProps) {
  const { custom, room, stream, telemetry } = snapshot

  const includeLocalTile =
    stream.isSharing &&
    !stream.ecoHidePreview &&
    !snapshot.popoutTiles.includes(0) &&
    (room.peers.filter((p) => p.isSharing).length === 0 || stream.showOwnPreviewInGrid)

  const rawTiles: Array<{
    tileId: number
    title: string
    subtitle: string
    volumePercent?: number
    muted?: boolean
  }> = []

  if (includeLocalTile) {
    rawTiles.push({
      tileId: 0,
      title: `${custom.displayName} (Sua Tela)`,
      subtitle: `${telemetry.localVideoSize[0]}×${telemetry.localVideoSize[1]} • ${telemetry.localFps} FPS • ${telemetry.localMbps.toFixed(1)} Mbps`,
    })
  }
  for (const p of room.peers) {
    if (p.isSharing && !snapshot.popoutTiles.includes(p.peerId)) {
      rawTiles.push({
        tileId: p.peerId,
        title: p.name,
        subtitle: `${p.videoSize[0]}×${p.videoSize[1]} • ${p.fps} FPS • ${p.mbps.toFixed(1)} Mbps`,
        volumePercent: p.volumePercent,
        muted: p.muted,
      })
    }
  }

  const orderedIds = [
    ...tileOrder.filter((id) => rawTiles.some((t) => t.tileId === id)),
    ...rawTiles.map((t) => t.tileId).filter((id) => !tileOrder.includes(id)),
  ]
  const visibleTiles = orderedIds
    .map((id) => rawTiles.find((t) => t.tileId === id))
    .filter((t): t is NonNullable<typeof t> => t !== undefined)

  const activeFocusId =
    focusedTileId !== null && visibleTiles.some((t) => t.tileId === focusedTileId)
      ? focusedTileId
      : null
  const displayedTiles =
    activeFocusId !== null ? visibleTiles.filter((t) => t.tileId === activeFocusId) : visibleTiles

  const moveTile = (tileId: number) => {
    if (orderedIds.length < 2) return
    const next = [...orderedIds]
    const idx = next.indexOf(tileId)
    if (idx === -1) return
    const swapIdx = (idx + 1) % next.length
    ;[next[idx], next[swapIdx]] = [next[swapIdx]!, next[idx]!]
    onSetTileOrder(next)
  }

  if (displayedTiles.length > 0) {
    return (
      <div
        className={`stream-grid ${
          displayedTiles.length === 1
            ? 'grid-cols-1'
            : displayedTiles.length <= 4
              ? 'grid-cols-2'
              : 'grid-cols-3'
        }`}
      >
        {displayedTiles.map((tile) => (
          <StreamTile
            key={tile.tileId}
            tileId={tile.tileId}
            title={tile.title}
            subtitle={tile.subtitle}
            isFocused={activeFocusId === tile.tileId}
            wsFps={wsStats.fpsByTile[tile.tileId] ?? 0}
            wsKbps={wsStats.kbpsByTile[tile.tileId] ?? 0}
            volumePercent={tile.volumePercent}
            muted={tile.muted}
            onAudioChange={
              tile.tileId > 0
                ? (vol, m) =>
                    void onCallRpc('set_peer_audio', {
                      peerId: tile.tileId,
                      volumePercent: vol,
                      muted: m,
                    })
                : undefined
            }
            onToggleFocus={
              visibleTiles.length > 1
                ? () => onSetFocusedTileId(activeFocusId === tile.tileId ? null : tile.tileId)
                : undefined
            }
            onMoveTile={visibleTiles.length > 1 ? () => moveTile(tile.tileId) : undefined}
            onPopout={() => {
              void onCallRpc('set_popout', { tileId: tile.tileId, popout: true })
              if (!isTauri())
                window.open(
                  `${window.location.pathname}?popout=${tile.tileId}`,
                  `screenz_popout_${tile.tileId}`,
                  'width=960,height=580',
                )
            }}
          />
        ))}
      </div>
    )
  }

  if (snapshot.popoutTiles.length > 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 rounded-2xl border border-dashed border-input p-8 text-center">
        <ArrowUpRight className="size-9 text-primary" />
        <h2 className="font-display text-2xl">Suas telas ganharam espaço.</h2>
        <p className="text-sm text-muted-foreground">
          As transmissões estão nas janelas separadas.
        </p>
        <Button onClick={() => void onCallRpc('set_popout', { clearAll: true })}>
          <ArrowDownToLine className="size-4" /> Trazer telas de volta
        </Button>
      </div>
    )
  }

  if (stream.isSharing && stream.ecoHidePreview) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 rounded-2xl border border-input bg-secondary p-8 text-center">
        <Radio className="size-9 text-primary" />
        <h2 className="font-display text-2xl">Sua transmissão continua ao vivo.</h2>
        <p className="text-xs text-muted-foreground">
          A prévia local está oculta para economizar recursos enquanto você joga.
        </p>
        <Button onClick={() => void onCallRpc('update_stream_settings', { ecoHidePreview: false })}>
          Mostrar minha prévia
        </Button>
      </div>
    )
  }

  return (
    <div className="empty-stage">
      <div className="welcome-stage">
        <div className="welcome-copy">
          <div className="eyebrow">
            {room.inRoom ? 'A sala está aberta' : 'Um lugar para estar junto'}
          </div>
          <h2 className="welcome-title">
            Sua tela,
            <br />
            <em>em boa companhia.</em>
          </h2>
          <p>
            {room.inRoom
              ? 'Compartilhe um aplicativo ou seu monitor. Quem está na sala acompanha com você.'
              : 'Entre em uma sala, compartilhe uma tela e acompanhe o que está acontecendo com a sua turma.'}
          </p>
          <Button
            className="h-11 gap-3 px-5 text-xs"
            onClick={custom.savedServers.length ? onOpenSharePicker : onOpenCreateSpace}
          >
            <MonitorUp className="size-4" />
            {custom.savedServers.length ? 'Escolher uma tela' : 'Criar meu primeiro espaço'}
            <ArrowUpRight className="size-4" />
          </Button>
        </div>
        <div className="signal-visual" aria-hidden="true">
          <div className="signal-orbit" />
          <div className="signal-orbit" />
          <div className="signal-orbit" />
          <div className="signal-core">
            <img src="/favicon.svg" alt="" />
          </div>
        </div>
      </div>
      {custom.savedServers.length > 0 && (
        <div className="stage-actions">
          <button type="button" className="stage-action" onClick={onOpenCreateSpace}>
            <Plus />
            <div>
              <strong>Criar um espaço</strong>
              <span>Um lugar para a sua turma.</span>
            </div>
            <ArrowUpRight />
          </button>
          <button type="button" className="stage-action" onClick={onOpenJoinSpace}>
            <KeyRound />
            <div>
              <strong>Entrar com convite</strong>
              <span>Seu encontro já tem endereço.</span>
            </div>
            <ArrowUpRight />
          </button>
        </div>
      )}
      <div className="stage-presence">
        <span className="flex min-w-0 items-center gap-2">
          <span className={`status-dot ${room.inRoom ? 'is-online' : ''}`} />
          {room.inRoom
            ? `${custom.displayName}${room.peers.length ? ` e mais ${room.peers.length} na sala` : ' está na sala'}`
            : custom.savedServers.length
              ? 'Escolha uma sala no painel ao lado'
              : 'Crie um espaço ou entre com um convite'}
        </span>
        <button
          type="button"
          className="flex items-center gap-2 rounded-sm hover:text-foreground"
          onClick={() =>
            void onCallRpc('update_stream_settings', { shareAudio: !stream.shareAudio })
          }
          title="Alternar compartilhamento do áudio da tela"
        >
          {stream.shareAudio ? <Volume2 className="size-3.5" /> : <VolumeX className="size-3.5" />}
          Áudio da tela {stream.shareAudio ? 'ativado' : 'desativado'}
        </button>
      </div>
    </div>
  )
}
