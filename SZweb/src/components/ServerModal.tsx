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
import type { RgbTuple } from '@/models/types'

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
    await onCreateServer(name.trim(), [88, 101, 242], rooms)
    setName('')
    onOpenChange(false)
  }

  const handleJoin = async () => {
    if (!inviteInput.trim()) return
    await onJoinServer(inviteInput.trim())
    setInviteInput('')
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Globe className="size-5 text-primary" /> Servidores do SZdisc
          </DialogTitle>
          <DialogDescription>
            Crie um servidor isolado ou cole o código de convite de um amigo.
          </DialogDescription>
        </DialogHeader>

        <Tabs defaultValue={initialTab} key={`${open}-${initialTab}`}>
          <TabsList className="w-full">
            <TabsTrigger value="create">
              <Plus className="size-3.5" /> Criar Servidor
            </TabsTrigger>
            <TabsTrigger value="join">
              <KeyRound className="size-3.5" /> Entrar com Convite
            </TabsTrigger>
          </TabsList>

          <TabsContent value="create" className="mt-3 space-y-3">
            <div>
              <label className="mb-1 block text-xs font-semibold text-muted-foreground">
                NOME DO SERVIDOR *
              </label>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Ex: Tropa do Valorant"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs font-semibold text-muted-foreground">
                SALAS INICIAIS (SEPARADAS POR VÍRGULA)
              </label>
              <Input value={roomsInput} onChange={(e) => setRoomsInput(e.target.value)} />
            </div>
            <Button
              variant="success"
              className="w-full"
              disabled={!name.trim()}
              onClick={() => void handleCreate()}
            >
              Criar Servidor no Rust
            </Button>
          </TabsContent>

          <TabsContent value="join" className="mt-3 space-y-3">
            <div>
              <label className="mb-1 block text-xs font-semibold text-muted-foreground">
                CONVITE OU CÓDIGO DO SERVIDOR *
              </label>
              <Input
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
              Entrar e Salvar Servidor
            </Button>
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}
