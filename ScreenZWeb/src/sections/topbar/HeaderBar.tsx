import {
  ArrowDownToLine,
  ArrowUpRight,
  Download,
  LayoutGrid,
  MonitorUp,
  Square,
} from 'lucide-react'

import { Button } from '@/components/ui/button'
import type { AppSnapshotDto } from '@/types/bridge'

interface HeaderBarProps {
  snapshot: AppSnapshotDto
  focusedTileId: number | null
  onUnfocus: () => void
  onDockAllPopouts: () => void
  onApplyUpdate: () => void
  onOpenSharePicker: () => void
  onStopSharing: () => void
}

export function HeaderBar({
  snapshot,
  focusedTileId,
  onUnfocus,
  onDockAllPopouts,
  onApplyUpdate,
  onOpenSharePicker,
  onStopSharing,
}: HeaderBarProps) {
  const { stream, room, custom } = snapshot
  const connectedSpace = custom.savedServers.find(
    (space) => space.serverId.toLowerCase() === room.connectedServerId.toLowerCase(),
  )

  return (
    <header className="studio-header">
      <div className="min-w-0">
        <div className="eyebrow mb-2">
          {room.inRoom ? (connectedSpace?.name ?? 'Espaço conectado') : 'Estúdio'}
        </div>
        <h1 className="studio-title truncate">
          {room.inRoom ? room.activeRoomName : 'Tudo pronto por aqui.'}
        </h1>
      </div>
      <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
        {snapshot.updateStatus.startsWith('UpdateAvailable:') && (
          <Button
            size="icon"
            variant="outline"
            title="Instalar atualização"
            aria-label="Instalar atualização"
            onClick={onApplyUpdate}
          >
            <Download className="size-4" />
          </Button>
        )}
        {focusedTileId !== null && (
          <Button
            size="icon"
            variant="outline"
            title="Ver todas as telas"
            aria-label="Ver todas as telas"
            onClick={onUnfocus}
          >
            <LayoutGrid className="size-4" />
          </Button>
        )}
        {snapshot.popoutTiles.length > 0 && (
          <Button
            size="icon"
            variant="outline"
            title="Trazer telas de volta"
            aria-label="Trazer telas de volta"
            onClick={onDockAllPopouts}
          >
            <ArrowDownToLine className="size-4" />
          </Button>
        )}
        {stream.isSharing && (
          <Button
            size="icon"
            variant="outline"
            className="text-destructive"
            aria-label="Parar transmissão"
            title="Parar transmissão"
            onClick={onStopSharing}
          >
            <Square className="size-3.5" />
          </Button>
        )}
        <Button className="gap-2.5 px-4 text-xs" onClick={onOpenSharePicker}>
          <MonitorUp className="size-4" />
          {stream.isSharing
            ? 'Trocar tela'
            : custom.savedServers.length
              ? 'Compartilhar tela'
              : 'Criar espaço'}
          <ArrowUpRight className="size-3.5 opacity-60" />
        </Button>
      </div>
    </header>
  )
}
