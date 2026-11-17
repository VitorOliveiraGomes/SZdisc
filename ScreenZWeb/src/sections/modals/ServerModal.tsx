import { useState } from 'react'
import { Globe, KeyRound, Plus } from 'lucide-react'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import type { RgbTuple } from '@/types/bridge'

interface ServerModalProps {
  open: boolean
  initialTab: 'create' | 'join'
  onOpenChange: (open: boolean) => void
  onCreateServer: (name: string, iconRgb: RgbTuple, rooms: string[]) => Promise<void>
  onJoinServer: (invite: string) => Promise<void>
}

export function ServerModal({
  open,
  initialTab,
  onOpenChange,
  onCreateServer,
  onJoinServer,
}: ServerModalProps) {
  const [name, setName] = useState('')
  const [roomsInput, setRoomsInput] = useState('Sala Geral, Jogos, Cinema')
  const [inviteInput, setInviteInput] = useState('')

  const handleCreate = async () => {
    if (!name.trim()) return
    const rooms = roomsInput
      .split(',')
      .map((r) => r.trim())
      .filter(Boolean)
    try {
      await onCreateServer(name.trim(), [213, 247, 128], rooms)
    } catch {
      return // A mensagem do RPC aparece no aviso global, mantendo os dados para nova tentativa.
    }
    setName('')
    onOpenChange(false)
  }

  const handleJoin = async () => {
    if (!inviteInput.trim()) return
    try {
      await onJoinServer(inviteInput.trim())
    } catch {
      return // Mantém o convite disponível para correção.
    }
    setInviteInput('')
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Globe className="size-5 text-primary" /> Um espaço para se encontrar
          </DialogTitle>
          <DialogDescription>
            Reúna sua turma em salas ou entre em um espaço com um convite.
          </DialogDescription>
        </DialogHeader>

        <Tabs defaultValue={initialTab} key={`${open}-${initialTab}`}>
          <TabsList className="w-full">
            <TabsTrigger value="create">
              <Plus className="size-3.5" /> Criar espaço
            </TabsTrigger>
            <TabsTrigger value="join">
              <KeyRound className="size-3.5" /> Entrar com Convite
            </TabsTrigger>
          </TabsList>

          <TabsContent value="create" className="mt-3 space-y-3">
            <div>
              <label
                htmlFor="space-name"
                className="mb-1 block text-xs font-semibold text-muted-foreground"
              >
                Nome do espaço *
              </label>
              <Input
                id="space-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="O ponto de encontro da sua turma"
              />
            </div>
            <div>
              <label
                htmlFor="space-rooms"
                className="mb-1 block text-xs font-semibold text-muted-foreground"
              >
                Salas iniciais · separadas por vírgulas
              </label>
              <Input
                id="space-rooms"
                value={roomsInput}
                onChange={(e) => setRoomsInput(e.target.value)}
              />
            </div>
            <Button
              variant="success"
              className="w-full"
              disabled={!name.trim()}
              onClick={() => void handleCreate()}
            >
              Criar espaço
            </Button>
          </TabsContent>

          <TabsContent value="join" className="mt-3 space-y-3">
            <div>
              <label
                htmlFor="space-invite"
                className="mb-1 block text-xs font-semibold text-muted-foreground"
              >
                Convite ou código do espaço *
              </label>
              <Input
                id="space-invite"
                value={inviteInput}
                onChange={(e) => setInviteInput(e.target.value)}
                placeholder="Ex: SZ-48291/Tropa/Sala Geral  ou  SZ-48291"
              />
            </div>
            <Button
              variant="success"
              className="w-full"
              disabled={!inviteInput.trim()}
              onClick={() => void handleJoin()}
            >
              Entrar no espaço
            </Button>
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}
