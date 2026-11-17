import { useEffect } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { StreamTile } from '@/components/StreamTile'
import type { AppSnapshotDto, BridgeConnectionMode, WsStreamStats } from '@/types/bridge'

interface PopoutStageProps {
  tileId: number
  snapshot: AppSnapshotDto
  mode: BridgeConnectionMode
  wsStats: WsStreamStats
  onCallRpc: (method: string, params?: Record<string, unknown>) => Promise<void>
}

export function PopoutStage({ tileId, snapshot, mode, wsStats, onCallRpc }: PopoutStageProps) {
  const peer = snapshot.room.peers.find((p) => p.peerId === tileId)
  const isLocal = tileId === 0
  const title = isLocal
    ? `${snapshot.custom.displayName} (Sua Tela)`
    : (peer?.name ?? `Stream #${tileId}`)

  useEffect(() => {
    document.title = `ScreenZ Pop-out — ${title}`
  }, [title])

  useEffect(() => {
    if (isTauri() && mode === 'rust' && !snapshot.popoutTiles.includes(tileId)) {
      void getCurrentWindow().close().catch(console.error)
    }
  }, [mode, snapshot.popoutTiles, tileId])

  return (
    <div className="h-screen w-screen bg-black p-2">
      <StreamTile
        tileId={tileId}
        title={title}
        subtitle={
          isLocal
            ? `${snapshot.telemetry.localVideoSize[0]}×${snapshot.telemetry.localVideoSize[1]}`
            : peer
              ? `${peer.videoSize[0]}×${peer.videoSize[1]}`
              : 'Pop-out'
        }
        isFocused={false}
        isPopout
        wsFps={wsStats.fpsByTile[tileId] ?? 0}
        wsKbps={wsStats.kbpsByTile[tileId] ?? 0}
        volumePercent={peer?.volumePercent}
        muted={peer?.muted}
        onAudioChange={
          peer
            ? (vol, m) =>
                void onCallRpc('set_peer_audio', {
                  peerId: peer.peerId,
                  volumePercent: vol,
                  muted: m,
                })
            : undefined
        }
      />
    </div>
  )
}
