import { useState } from 'react'
import {
  ArrowRight,
  AudioLines,
  Check,
  Copy,
  LogOut,
  Plus,
  Radio,
  Trash2,
  Users,
  X,
} from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import type {
  RoomSnapshotDto,
  ServerProfileDto,
  StreamSettingsDto,
  UserCustomizationDto,
} from '@/types/bridge'

interface RoomsPanelProps {
  selectedServer?: ServerProfileDto
  custom: UserCustomizationDto
  room: RoomSnapshotDto
  stream: StreamSettingsDto
  onCallRpc: (method: string, params?: Record<string, unknown>) => Promise<void>
  onOpenCreateSpace: () => void
  onOpenJoinSpace: () => void
}

export function RoomsPanel({
  selectedServer,
  custom,
  room,
  stream,
  onCallRpc,
  onOpenCreateSpace,
  onOpenJoinSpace,
}: RoomsPanelProps) {
  const [newRoomName, setNewRoomName] = useState('')
  const [copiedInvite, setCopiedInvite] = useState('')
  const [copyError, setCopyError] = useState('')
  const copied = Boolean(selectedServer && copiedInvite === selectedServer.inviteToken)

  const copyInvite = async () => {
    if (!selectedServer) return
    try {
      await navigator.clipboard.writeText(selectedServer.inviteToken)
      setCopiedInvite(selectedServer.inviteToken)
      setCopyError('')
    } catch {
      setCopyError('Não foi possível copiar. O convite está disponível abaixo.')
    }
  }

  const createRoom = async () => {
    if (!selectedServer || !newRoomName.trim()) return
    try {
      await onCallRpc('create_room', {
        serverId: selectedServer.serverId,
        roomName: newRoomName.trim(),
      })
      setNewRoomName('')
    } catch {
      // O aviso do RPC mantém o nome disponível para nova tentativa.
    }
  }

  return (
    <aside className="rooms-panel" aria-label="Salas e participantes">
      <div className="rooms-heading">
        <div className="eyebrow mb-3">Seu espaço</div>
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <h2 className="truncate font-display text-xl font-semibold tracking-tight">
              {selectedServer?.name ?? 'Boa companhia começa aqui'}
            </h2>
            <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
              {selectedServer
                ? `${selectedServer.rooms.length} salas para se encontrar`
                : 'Crie um espaço ou entre com o convite de alguém.'}
            </p>
          </div>
          {selectedServer && (
            <Button
              size="icon"
              variant="ghost"
              className="size-7 shrink-0 text-muted-foreground"
              title="Remover espaço da sua lista"
              aria-label="Remover espaço da sua lista"
              onClick={() => void onCallRpc('remove_server', { serverId: selectedServer.serverId })}
            >
              <Trash2 className="size-3.5" />
            </Button>
          )}
        </div>
        {selectedServer && (
          <Button
            variant="outline"
            className="mt-5 h-9 w-full text-xs"
            onClick={() => void copyInvite()}
          >
            {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            {copied ? 'Convite copiado' : 'Convidar alguém'}
          </Button>
        )}
        {copyError && (
          <div className="mt-2 space-y-2 text-xs">
            <p role="alert" className="text-destructive">
              {copyError}
            </p>
            <Input
              aria-label="Convite do espaço"
              value={selectedServer?.inviteToken ?? ''}
              readOnly
              onFocus={(event) => event.target.select()}
            />
          </div>
        )}
      </div>
      {selectedServer ? (
        <>
          <div className="flex items-center justify-between px-6 pb-3">
            <span className="eyebrow">Salas</span>
            <Users className="size-3.5 text-muted-foreground" aria-hidden="true" />
          </div>
          <div className="room-list">
            {selectedServer.rooms.map((roomName) => {
              const isCurrent =
                room.inRoom &&
                room.connectedServerId.toLowerCase() === selectedServer.serverId.toLowerCase() &&
                room.activeRoomName.toLowerCase() === roomName.toLowerCase()
              const members = room.serverMembers.filter(
                (member) =>
                  member.serverId.toLowerCase() === selectedServer.serverId.toLowerCase() &&
                  member.currentRoom.toLowerCase() === roomName.toLowerCase(),
              )
              return (
                <div key={roomName} className={`room-item group ${isCurrent ? 'is-current' : ''}`}>
                  <div className="flex items-center">
                    <button
                      type="button"
                      className="room-select"
                      aria-current={isCurrent ? 'true' : undefined}
                      onClick={() =>
                        void onCallRpc('enter_room', {
                          serverId: selectedServer.serverId,
                          roomName,
                        })
                      }
                    >
                      <span className="room-symbol">
                        <AudioLines className="size-4" />
                      </span>
                      <span className="truncate">{roomName}</span>
                      {isCurrent && <span className="ml-auto status-dot is-online" />}
                    </button>
                    {!isCurrent && selectedServer.rooms.length > 1 && (
                      <Button
                        size="icon"
                        variant="ghost"
                        className="size-6 shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                        aria-label={`Remover sala ${roomName}`}
                        title={`Remover sala ${roomName}`}
                        onClick={() =>
                          void onCallRpc('delete_room', {
                            serverId: selectedServer.serverId,
                            roomName,
                          })
                        }
                      >
                        <X className="size-3" />
                      </Button>
                    )}
                  </div>
                  {isCurrent && (
                    <div className="room-member">
                      <span className="member-initial">{custom.displayName[0]?.toUpperCase()}</span>
                      <span className="min-w-0 flex-1 truncate">
                        {custom.displayName} <span className="opacity-60">· você</span>
                      </span>
                      {stream.isSharing && (
                        <Radio className="size-3 text-primary" aria-label="Transmitindo" />
                      )}
                    </div>
                  )}
                  {members.map((member) => (
                    <div key={member.peerId} className="room-member">
                      <span className="member-initial">{member.name[0]?.toUpperCase()}</span>
                      <span className="min-w-0 flex-1 truncate">{member.name}</span>
                      {member.isSharing && (
                        <Radio className="size-3 text-primary" aria-label="Transmitindo" />
                      )}
                    </div>
                  ))}
                </div>
              )
            })}
            <form
              className="mt-5 flex items-center gap-2 px-1"
              onSubmit={(event) => {
                event.preventDefault()
                void createRoom()
              }}
            >
              <Input
                value={newRoomName}
                onChange={(event) => setNewRoomName(event.target.value)}
                placeholder="Uma nova sala..."
                aria-label="Nome da nova sala"
                className="h-9 min-w-0 text-xs"
              />
              <Button
                type="submit"
                size="icon"
                variant="secondary"
                className="size-9 shrink-0"
                disabled={!newRoomName.trim()}
                aria-label="Criar sala"
              >
                <Plus className="size-4" />
              </Button>
            </form>
          </div>
        </>
      ) : (
        <div className="room-list space-y-3 px-6">
          <Button className="w-full justify-between text-xs" onClick={onOpenCreateSpace}>
            Criar meu espaço <Plus className="size-4" />
          </Button>
          <Button
            variant="ghost"
            className="w-full justify-between text-xs"
            onClick={onOpenJoinSpace}
          >
            Tenho um convite <ArrowRight className="size-4" />
          </Button>
        </div>
      )}
      <div className="panel-session">
        <div className="mb-2 flex items-center gap-2 text-xs font-medium">
          <span className={`status-dot ${room.inRoom ? 'is-online' : ''}`} />
          {room.inRoom ? 'Você está na sala' : 'Ainda fora das salas'}
        </div>
        {room.inRoom ? (
          <div className="flex items-center justify-between gap-2">
            <span className="truncate text-xs text-muted-foreground">{room.activeRoomName}</span>
            <Button
              size="icon"
              variant="ghost"
              className="size-7 shrink-0 text-destructive"
              title="Sair da sala"
              aria-label="Sair da sala"
              onClick={() => void onCallRpc('leave_room')}
            >
              <LogOut className="size-3.5" />
            </Button>
          </div>
        ) : (
          <p className="text-[11px] leading-relaxed text-muted-foreground">
            Escolha uma sala para compartilhar telas e acompanhar quem está por lá.
          </p>
        )}
      </div>
    </aside>
  )
}
