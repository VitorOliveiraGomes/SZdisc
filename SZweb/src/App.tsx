import { useEffect, useMemo, useState } from 'react'
import {
  ArrowDownToLine,
  Copy,
  Image,
  KeyRound,
  MonitorUp,
  PhoneOff,
  Plus,
  Settings,
  Square,
  Trash2,
  Volume2,
} from 'lucide-react'

import { BridgeInspectorDialog } from '@/components/BridgeInspectorDialog'
import { Header } from '@/components/Header'
import { ProfileCard } from '@/components/ProfileCard'
import { ServerModal } from '@/components/ServerModal'
import { SettingsDialog } from '@/components/SettingsDialog'
import { SharePickerDialog } from '@/components/SharePickerDialog'
import { StreamTile } from '@/components/StreamTile'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import type {
  AppSnapshotDto,
  RgbTuple,
  RpcLogEntry,
  WsStreamStats,
} from '@/models/types'
import {
  backendBridge,
  createInitialMockSnapshot,
  type BridgeConnectionMode,
} from '@/services/backendBridge'
import { applyCustomizationToCssVars, rgbToCss } from '@/services/themeSync'
import { videoStreamClient } from '@/services/videoStreamClient'

function App() {
  const popoutTileParam = useMemo(() => {
    const raw = new URLSearchParams(window.location.search).get('popout')
    if (raw === null) return null
    const parsed = Number(raw)
    return Number.isFinite(parsed) ? parsed : null
  }, [])

  const [snapshot, setSnapshot] = useState<AppSnapshotDto>(() => createInitialMockSnapshot())
  const [mode, setMode] = useState<BridgeConnectionMode>('mock')
  const [logs, setLogs] = useState<RpcLogEntry[]>([])
  const [wsStats, setWsStats] = useState<WsStreamStats>({
    connected: false,
    totalFrames: 0,
    fpsByTile: {},
    kbpsByTile: {},
  })

  const [tileOrder, setTileOrder] = useState<number[]>([])
  const [focusedTileId, setFocusedTileId] = useState<number | null>(null)
  const [showProfilePopup, setShowProfilePopup] = useState(false)
  const [newRoomName, setNewRoomName] = useState('')
  const [onboardingName, setOnboardingName] = useState('')
  const [onboardingLogin, setOnboardingLogin] = useState('')
  const [onboardingStatus, setOnboardingStatus] = useState('')
  const [serverModal, setServerModal] = useState<{ open: boolean; tab: 'create' | 'join' }>({
    open: false,
    tab: 'create',
  })
  const [shareOpen, setShareOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [inspectorOpen, setInspectorOpen] = useState(false)

  const syncFromRust = async () => {
    const next = await backendBridge.fetchState()
    setSnapshot(next)
    setMode(backendBridge.getMode())
    applyCustomizationToCssVars(next.custom)
  }

  useEffect(() => {
    videoStreamClient.start(popoutTileParam)
    const unsubWs = videoStreamClient.subscribeStats(setWsStats)
    const unsubLogs = backendBridge.subscribeLogs((entries, connMode) => {
      setLogs(entries)
      setMode(connMode)
    })

    void syncFromRust()
    const pollId = window.setInterval(() => {
      void syncFromRust()
    }, 500)

    return () => {
      window.clearInterval(pollId)
      unsubWs()
      unsubLogs()
      videoStreamClient.stop()
    }
  }, [popoutTileParam])

  const callRpc = async (method: string, params: Record<string, unknown> = {}) => {
    const next = await backendBridge.invoke<AppSnapshotDto>(method, params)
    setSnapshot(next)
    setMode(backendBridge.getMode())
    applyCustomizationToCssVars(next.custom)
  }

  // Modo Pop-out dedicado (?popout=<tileId>)
  if (popoutTileParam !== null) {
    const peer = snapshot.room.peers.find((p) => p.peerId === popoutTileParam)
    const isLocal = popoutTileParam === 0
    return (
      <div className="h-screen w-screen bg-black p-2">
        <StreamTile
          tileId={popoutTileParam}
          title={isLocal ? `${snapshot.custom.displayName} (Sua Tela)` : peer?.name ?? `Stream #${popoutTileParam}`}
          subtitle={
            isLocal
              ? `${snapshot.telemetry.localVideoSize[0]}×${snapshot.telemetry.localVideoSize[1]}`
              : peer
                ? `${peer.videoSize[0]}×${peer.videoSize[1]}`
                : 'Pop-out'
          }
          isFocused={false}
          isPopout
          wsFps={wsStats.fpsByTile[popoutTileParam] ?? 0}
          wsKbps={wsStats.kbpsByTile[popoutTileParam] ?? 0}
          volumePercent={peer?.volumePercent}
          muted={peer?.muted}
          onAudioChange={
            peer
              ? (vol, m) => void callRpc('set_peer_audio', { peerId: peer.peerId, volumePercent: vol, muted: m })
              : undefined
          }
        />
      </div>
    )
  }

  // Tela de Onboarding se conta ainda não foi criada no Rust
  if (!snapshot.custom.accountCreated) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background p-4">
        <Card className="w-full max-w-md border-primary/50">
          <CardHeader className="text-center">
            <CardTitle className="text-xl">● Bem-vindo ao SZdisc</CardTitle>
            <CardDescription>
              Crie sua conta para criar ou participar de Servidores com seus amigos.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex flex-col items-center gap-2">
              <Avatar className="size-18 border-2 border-primary">
                {snapshot.custom.avatarDataUrl && (
                  <AvatarImage src={snapshot.custom.avatarDataUrl} alt="Avatar" />
                )}
                <AvatarFallback style={{ backgroundColor: rgbToCss(snapshot.custom.avatarBg) }}>
                  {(onboardingName.trim()[0] ?? '?').toUpperCase()}
                </AvatarFallback>
              </Avatar>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => void callRpc('pick_profile_image', { kind: 'avatar' })}
              >
                <Image className="size-3.5" /> Escolher Foto de Perfil (Opcional)
              </Button>
            </div>
            <Input
              value={onboardingName}
              onChange={(e) => setOnboardingName(e.target.value)}
              placeholder="Seu nome de exibição * (Ex: Pedro, Vitor...)"
            />
            <Input
              value={onboardingLogin}
              onChange={(e) => setOnboardingLogin(e.target.value)}
              placeholder="Nome de usuário @login (opcional)"
            />
            <Input
              value={onboardingStatus}
              onChange={(e) => setOnboardingStatus(e.target.value)}
              placeholder="Status personalizado (Ex: Jogando Valorant)"
            />
            <Button
              variant="success"
              className="w-full"
              disabled={!onboardingName.trim()}
              onClick={() =>
                void callRpc('complete_onboarding', {
                  displayName: onboardingName.trim(),
                  usernameLogin: onboardingLogin.trim(),
                  customStatus: onboardingStatus.trim(),
                })
              }
            >
              🚀 Criar Conta e Entrar no SZdisc
            </Button>
          </CardContent>
        </Card>
      </div>
    )
  }

  const { custom, room, stream, telemetry } = snapshot
  const selectedServer =
    custom.savedServers.find(
      (s) => s.serverId.toLowerCase() === custom.selectedServerId.toLowerCase(),
    ) ?? custom.savedServers[0]

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
    setTileOrder(next)
  }

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      {/* COLUNA 1: Barra de Servidores */}
      <aside
        className="flex w-17 shrink-0 flex-col items-center gap-2 border-r border-border py-3"
        style={{ backgroundColor: 'var(--sz-rail)' }}
      >
        <ScrollArea className="w-full flex-1">
          <div className="flex flex-col items-center gap-2 px-2">
            {custom.savedServers.map((srv) => {
              const isSelected = selectedServer?.serverId === srv.serverId
              const isVoiceHere =
                room.inRoom && room.connectedServerId.toLowerCase() === srv.serverId.toLowerCase()
              const initials =
                srv.name
                  .split(/\s+/)
                  .map((w) => w[0])
                  .join('')
                  .slice(0, 2)
                  .toUpperCase() || 'SZ'

              return (
                <button
                  key={srv.serverId}
                  type="button"
                  onClick={() => void callRpc('select_server', { serverId: srv.serverId })}
                  title={`${srv.name} (${srv.serverId})`}
                  className={`relative flex size-11 items-center justify-center font-bold text-white transition-all cursor-pointer ${
                    isSelected ? 'rounded-xl ring-2 ring-white' : 'rounded-full hover:rounded-xl'
                  }`}
                  style={{ backgroundColor: rgbToCss(srv.iconRgb) }}
                >
                  <span className="text-sm">{initials}</span>
                  {isVoiceHere && (
                    <span className="absolute -right-0.5 -bottom-0.5 size-3 rounded-full border-2 border-background bg-emerald-500" />
                  )}
                </button>
              )
            })}
          </div>
        </ScrollArea>

        <Separator className="w-8" />

        <Button
          size="icon"
          variant="secondary"
          className="size-11 rounded-full text-emerald-400 hover:rounded-xl"
          title="Criar Novo Servidor"
          onClick={() => setServerModal({ open: true, tab: 'create' })}
        >
          <Plus className="size-5" />
        </Button>

        <Button
          size="icon"
          variant="secondary"
          className="size-10 rounded-full hover:rounded-xl"
          title="Entrar em Servidor com Convite"
          onClick={() => setServerModal({ open: true, tab: 'join' })}
        >
          <KeyRound className="size-4" />
        </Button>
      </aside>

      {/* COLUNA 2: Salas + Voz Conectada + Usuário */}
      <aside
        className="flex w-68 shrink-0 flex-col border-r border-border"
        style={{ backgroundColor: 'var(--sz-sidebar)' }}
      >
        <div className="flex-1 overflow-hidden p-3 flex flex-col gap-3">
          {selectedServer ? (
            <>
              <div className="flex items-center justify-between gap-2 rounded-lg border border-border bg-card p-2.5">
                <div className="min-w-0">
                  <div className="truncate text-sm font-bold">{selectedServer.name}</div>
                  <div className="text-[11px] text-muted-foreground">
                    ID: {selectedServer.serverId}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <Button
                    size="sm"
                    className="h-7 px-2 text-xs"
                    onClick={() => {
                      void navigator.clipboard?.writeText(selectedServer.inviteToken)
                      void callRpc('show_toast', {
                        message: `Convite de '${selectedServer.name}' copiado!`,
                      })
                    }}
                  >
                    <Copy className="size-3" /> Convidar
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-7 text-muted-foreground hover:text-destructive"
                    onClick={() =>
                      void callRpc('remove_server', { serverId: selectedServer.serverId })
                    }
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                </div>
              </div>

              <div className="flex gap-1.5">
                <Input
                  value={newRoomName}
                  onChange={(e) => setNewRoomName(e.target.value)}
                  placeholder="Nova sala..."
                  className="h-8 text-xs"
                />
                <Button
                  size="sm"
                  className="h-8 px-2.5 text-xs"
                  disabled={!newRoomName.trim()}
                  onClick={() => {
                    void callRpc('create_room', {
                      serverId: selectedServer.serverId,
                      roomName: newRoomName.trim(),
                    })
                    setNewRoomName('')
                  }}
                >
                  Criar
                </Button>
              </div>

              <ScrollArea className="flex-1 pr-1">
                <div className="flex flex-col gap-1.5">
                  {selectedServer.rooms.map((roomName) => {
                    const isCurrent =
                      room.inRoom &&
                      room.connectedServerId.toLowerCase() ===
                        selectedServer.serverId.toLowerCase() &&
                      room.activeRoomName.toLowerCase() === roomName.toLowerCase()

                    const membersInRoom = room.serverMembers.filter(
                      (m) =>
                        m.serverId.toLowerCase() === selectedServer.serverId.toLowerCase() &&
                        m.currentRoom.toLowerCase() === roomName.toLowerCase(),
                    )

                    return (
                      <div key={roomName} className="space-y-1">
                        <div
                          className={`flex items-center justify-between rounded-md border px-2.5 py-2 text-xs transition-colors ${
                            isCurrent
                              ? 'border-primary bg-primary/15 font-semibold'
                              : 'border-border/50 bg-card/70 hover:bg-card'
                          }`}
                        >
                          <button
                            type="button"
                            className="flex flex-1 items-center gap-1.5 text-left cursor-pointer"
                            onClick={() =>
                              void callRpc('enter_room', {
                                serverId: selectedServer.serverId,
                                roomName,
                              })
                            }
                          >
                            <Volume2
                              className={`size-3.5 ${isCurrent ? 'text-emerald-400' : 'text-muted-foreground'}`}
                            />
                            <span className="truncate">{roomName}</span>
                          </button>

                          {!isCurrent && selectedServer.rooms.length > 1 && (
                            <Button
                              size="icon"
                              variant="ghost"
                              className="size-5 text-muted-foreground"
                              onClick={() =>
                                void callRpc('delete_room', {
                                  serverId: selectedServer.serverId,
                                  roomName,
                                })
                              }
                            >
                              ×
                            </Button>
                          )}
                        </div>

                        {isCurrent && (
                          <div className="ml-4 flex items-center justify-between rounded-md bg-card/50 px-2 py-1 text-[11px]">
                            <span className="truncate font-medium">{custom.displayName} (Você)</span>
                            {stream.isSharing && (
                              <Badge variant="live" className="px-1 py-0 text-[9px]">
                                LIVE
                              </Badge>
                            )}
                          </div>
                        )}

                        {membersInRoom.map((m) => (
                          <div
                            key={m.peerId}
                            className="ml-4 flex items-center justify-between rounded-md bg-card/50 px-2 py-1 text-[11px]"
                          >
                            <span className="truncate">{m.name}</span>
                            {m.isSharing && (
                              <Badge variant="live" className="px-1 py-0 text-[9px]">
                                LIVE
                              </Badge>
                            )}
                          </div>
                        ))}
                      </div>
                    )
                  })}
                </div>
              </ScrollArea>
            </>
          ) : (
            <div className="flex flex-1 flex-col items-center justify-center gap-2 text-center">
              <div className="text-sm font-semibold">Nenhum Servidor</div>
              <Button size="sm" onClick={() => setServerModal({ open: true, tab: 'create' })}>
                Criar Servidor
              </Button>
            </div>
          )}
        </div>

        {/* Painel de Voz Conectada */}
        <div className="border-t border-border bg-black/20 p-2.5 space-y-2">
          {room.inRoom ? (
            <>
              <div className="truncate text-xs font-semibold text-emerald-400">
                🟢 {room.activeRoomName}
              </div>
              <div className="grid grid-cols-2 gap-1.5">
                <Button
                  size="sm"
                  variant={stream.isSharing ? 'default' : 'secondary'}
                  className="h-8 text-xs"
                  onClick={() => setShareOpen(true)}
                >
                  <MonitorUp className="size-3.5" /> {stream.isSharing ? 'Trocar' : 'Transmitir'}
                </Button>
                <Button
                  size="sm"
                  variant="destructive"
                  className="h-8 text-xs"
                  onClick={() => void callRpc('leave_room')}
                >
                  <PhoneOff className="size-3.5" /> Sair
                </Button>
              </div>
              {stream.isSharing && (
                <Button
                  size="sm"
                  variant="destructive"
                  className="h-7 w-full text-xs"
                  onClick={() => void callRpc('stop_sharing')}
                >
                  <Square className="size-3" /> Parar Transmissão
                </Button>
              )}
            </>
          ) : (
            <div className="text-[11px] text-muted-foreground">
              Clique em uma sala acima para conectar.
            </div>
          )}
        </div>

        {/* Rodapé do Usuário */}
        <div className="flex h-14 items-center justify-between border-t border-border bg-black/35 px-3">
          <button
            type="button"
            onClick={() => setShowProfilePopup((prev) => !prev)}
            className="flex min-w-0 flex-1 items-center gap-2.5 text-left cursor-pointer"
          >
            <Avatar className="size-9 border border-primary">
              {custom.avatarDataUrl && (
                <AvatarImage src={custom.avatarDataUrl} alt={custom.displayName} />
              )}
              <AvatarFallback style={{ backgroundColor: rgbToCss(custom.avatarBg) }}>
                {(custom.displayName.trim()[0] ?? '?').toUpperCase()}
              </AvatarFallback>
            </Avatar>
            <div className="min-w-0">
              <div className="truncate text-xs font-bold" style={{ color: rgbToCss(custom.nameColorRgb) }}>
                {custom.displayName}
              </div>
              <div className="truncate text-[10px] text-muted-foreground">
                {stream.isSharing
                  ? `🔴 ${stream.resolution}`
                  : room.inRoom
                    ? `🟢 ${room.activeRoomName}`
                    : '⚪ Fora das salas'}
              </div>
            </div>
          </button>
          <Button size="icon" variant="ghost" className="size-8 shrink-0" onClick={() => setSettingsOpen(true)}>
            <Settings className="size-4" />
          </Button>
        </div>
      </aside>

      {/* COLUNA 3: TopBar + Palco Central */}
      <main className="flex min-w-0 flex-1 flex-col">
        <Header
          snapshot={snapshot}
          mode={mode}
          wsStats={wsStats}
          focusedTileId={activeFocusId}
          onUnfocus={() => setFocusedTileId(null)}
          onDockAllPopouts={() => void callRpc('set_popout', { clearAll: true })}
          onUpdateStream={(patch) => void callRpc('update_stream_settings', patch)}
          onApplyUpdate={() => void callRpc('apply_update')}
          onOpenSettings={() => setSettingsOpen(true)}
          onOpenInspector={() => setInspectorOpen(true)}
        />

        <section className="relative flex-1 overflow-hidden p-3" style={{ backgroundColor: 'var(--sz-stage)' }}>
          {displayedTiles.length > 0 ? (
            <div
              className={`grid h-full w-full gap-3 ${
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
                          void callRpc('set_peer_audio', {
                            peerId: tile.tileId,
                            volumePercent: vol,
                            muted: m,
                          })
                      : undefined
                  }
                  onToggleFocus={
                    visibleTiles.length > 1
                      ? () => setFocusedTileId(activeFocusId === tile.tileId ? null : tile.tileId)
                      : undefined
                  }
                  onMoveTile={visibleTiles.length > 1 ? () => moveTile(tile.tileId) : undefined}
                  onPopout={() => {
                    void callRpc('set_popout', { tileId: tile.tileId, popout: true })
                    window.open(
                      `${window.location.pathname}?popout=${tile.tileId}`,
                      `szdisc_popout_${tile.tileId}`,
                      'width=960,height=580',
                    )
                  }}
                />
              ))}
            </div>
          ) : snapshot.popoutTiles.length > 0 ? (
            <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
              <div className="text-base font-semibold text-primary">
                🖥️ Transmissões abertas em janelas separadas (Outro Monitor)
              </div>
              <Button onClick={() => void callRpc('set_popout', { clearAll: true })}>
                <ArrowDownToLine className="size-4" /> Trazer Todas as Telas de Volta
              </Button>
            </div>
          ) : stream.isSharing && stream.ecoHidePreview ? (
            <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
              <div className="text-base font-bold text-emerald-400">
                ● TRANSMITINDO AO VIVO NA SALA (MODO JOGO ATIVO)
              </div>
              <p className="text-xs text-muted-foreground">
                A prévia local está oculta para economizar recursos enquanto você joga.
              </p>
              <Button onClick={() => void callRpc('update_stream_settings', { ecoHidePreview: false })}>
                Mostrar Minha Tela Novamente
              </Button>
            </div>
          ) : (
            <div className="flex h-full flex-col items-center justify-center gap-5 text-center">
              <ProfileCard custom={custom} isSharing={false} />

              <Card className="max-w-lg border-primary/40">
                <CardHeader>
                  <CardTitle className="flex items-center justify-center gap-2 text-lg">
                    {room.inRoom
                      ? `Conectado na sala: ${room.activeRoomName}`
                      : 'SZdisc — React + Vite + shadcn/ui'}
                    <Badge variant={mode === 'rust' ? 'success' : 'outline'}>
                      {mode === 'rust' ? 'Rust Conectado' : 'Mock Preview'}
                    </Badge>
                  </CardTitle>
                  <CardDescription>
                    {room.inRoom
                      ? 'Clique em Compartilhar Tela para transmitir em 60 FPS pelo WebSocket binário.'
                      : 'Entre em uma sala à esquerda ou crie/entre em um servidor para começar.'}
                  </CardDescription>
                </CardHeader>
                <CardContent className="flex flex-wrap justify-center gap-2">
                  <Button variant="success" onClick={() => setShareOpen(true)}>
                    <MonitorUp className="size-4" /> Compartilhar Tela
                  </Button>
                  <Button onClick={() => setServerModal({ open: true, tab: 'create' })}>
                    <Plus className="size-4" /> Criar / Entrar Servidor
                  </Button>
                  <Button variant="secondary" onClick={() => setInspectorOpen(true)}>
                    Testar Ponte RPC/WS
                  </Button>
                </CardContent>
              </Card>
            </div>
          )}

          {showProfilePopup && (
            <div className="fixed bottom-16 left-20 z-40">
              <ProfileCard custom={custom} isSharing={stream.isSharing} />
            </div>
          )}

          {snapshot.activeToast && (
            <div className="pointer-events-none fixed right-4 bottom-4 z-50 rounded-lg border border-primary bg-card px-3.5 py-2 text-xs font-medium shadow-xl">
              {snapshot.activeToast}
            </div>
          )}
        </section>
      </main>

      <ServerModal
        open={serverModal.open}
        initialTab={serverModal.tab}
        onOpenChange={(open) => setServerModal((prev) => ({ ...prev, open }))}
        onCreateServer={(name, iconRgb: RgbTuple, rooms) =>
          callRpc('create_server', { name, iconRgb, rooms })
        }
        onJoinServer={(invite) => callRpc('join_server', { invite })}
      />

      <SharePickerDialog
        open={shareOpen}
        onOpenChange={setShareOpen}
        stream={stream}
        onUpdateStream={(patch) => callRpc('update_stream_settings', patch)}
        onStartSharing={(sourceId) => callRpc('start_sharing', { sourceId })}
      />

      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        custom={custom}
        stream={stream}
        onSaveProfile={(patch) => callRpc('save_profile', patch)}
        onPickProfileImage={(kind) => callRpc('pick_profile_image', { kind })}
        onClearProfileImage={(kind) => callRpc('clear_profile_image', { kind })}
        onApplyPreset={(presetCode) => callRpc('apply_theme_preset', { presetCode })}
        onSaveAppearance={(patch) => callRpc('save_appearance', patch)}
        onUpdateStream={(patch) => callRpc('update_stream_settings', patch)}
        onLogout={() => callRpc('logout')}
      />

      <BridgeInspectorDialog
        open={inspectorOpen}
        onOpenChange={setInspectorOpen}
        mode={mode}
        snapshot={snapshot}
        wsStats={wsStats}
        logs={logs}
        onRefreshState={() => void syncFromRust()}
        onSendToast={(message) => void callRpc('show_toast', { message })}
        onCheckUpdates={() => void callRpc('check_updates')}
      />
    </div>
  )
}

export default App
