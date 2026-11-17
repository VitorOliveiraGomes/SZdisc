import { KeyRound, Plus } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { rgbToCss } from '@/services/themeSync'
import type { RoomSnapshotDto, ServerProfileDto } from '@/types/bridge'

interface SpacesNavProps {
  servers: ServerProfileDto[]
  selectedServerId?: string
  room: RoomSnapshotDto
  onSelectServer: (serverId: string) => void
  onOpenCreateServer: () => void
  onOpenJoinServer: () => void
}

export function SpacesNav({
  servers,
  selectedServerId,
  room,
  onSelectServer,
  onOpenCreateServer,
  onOpenJoinServer,
}: SpacesNavProps) {
  return (
    <nav className="spaces-nav" aria-label="Espaços">
      <div className="space-tabs">
        {servers.length === 0 && (
          <span className="whitespace-nowrap text-xs text-muted-foreground">
            Seus espaços aparecem aqui
          </span>
        )}
        {servers.map((server) => {
          const connected =
            room.inRoom && room.connectedServerId.toLowerCase() === server.serverId.toLowerCase()
          return (
            <button
              key={server.serverId}
              type="button"
              className="space-tab"
              aria-current={
                selectedServerId?.toLowerCase() === server.serverId.toLowerCase()
                  ? 'page'
                  : undefined
              }
              onClick={() => onSelectServer(server.serverId)}
              title={server.name}
            >
              <span
                className="space-indicator"
                style={{ backgroundColor: rgbToCss(server.iconRgb) }}
              />
              <span className="max-w-36 truncate">{server.name}</span>
              {connected && <span className="status-dot is-online" aria-label="Conectado" />}
            </button>
          )
        })}
      </div>
      <div className="flex shrink-0 items-center gap-1 border-l border-border pl-2">
        <Button
          size="icon"
          variant="ghost"
          className="size-8"
          title="Criar espaço"
          aria-label="Criar espaço"
          onClick={onOpenCreateServer}
        >
          <Plus className="size-4" />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          className="size-8"
          title="Entrar com convite"
          aria-label="Entrar com convite"
          onClick={onOpenJoinServer}
        >
          <KeyRound className="size-4" />
        </Button>
      </div>
    </nav>
  )
}
