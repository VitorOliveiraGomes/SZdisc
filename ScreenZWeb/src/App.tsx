import { useEffect, useMemo, useState } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ChevronDown, Settings, X } from 'lucide-react'

import { Brand } from '@/components/Brand'
import { ProfileCard } from '@/components/ProfileCard'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { useBridgeState } from '@/hooks/useBridgeState'
import { BridgeInspectorDialog } from '@/sections/modals/BridgeInspectorDialog'
import { ServerModal } from '@/sections/modals/ServerModal'
import { SharePickerDialog } from '@/sections/modals/SharePickerDialog'
import { SettingsDialog } from '@/sections/modals/settings/SettingsDialog'
import { OnboardingScreen } from '@/sections/onboarding/OnboardingScreen'
import { PopoutStage } from '@/sections/popout/PopoutStage'
import { SpacesNav } from '@/sections/navigation/SpacesNav'
import { RoomsPanel } from '@/sections/rooms/RoomsPanel'
import { SessionBar } from '@/sections/stage/SessionBar'
import { StreamStage } from '@/sections/stage/StreamStage'
import { HeaderBar } from '@/sections/topbar/HeaderBar'
import type { RgbTuple } from '@/types/bridge'

function App() {
  const popoutTileParam = useMemo(() => {
    const raw = new URLSearchParams(window.location.search).get('popout')
    if (raw === null) return null
    const parsed = Number(raw)
    return /^\d+$/.test(raw) && Number.isInteger(parsed) && parsed <= 0xffffffff ? parsed : null
  }, [])

  const { snapshot, mode, logs, wsStats, syncFromRust, callRpc, rpcError, clearRpcError } =
    useBridgeState(popoutTileParam)

  const [tileOrder, setTileOrder] = useState<number[]>([])
  const [focusedTileId, setFocusedTileId] = useState<number | null>(null)
  const [showProfilePopup, setShowProfilePopup] = useState(false)
  const [serverModal, setServerModal] = useState<{ open: boolean; tab: 'create' | 'join' }>({
    open: false,
    tab: 'create',
  })
  const [shareOpen, setShareOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [inspectorOpen, setInspectorOpen] = useState(false)
  useEffect(() => {
    if (!isTauri()) return
    const syncFullscreen = () => {
      void getCurrentWindow()
        .setFullscreen(Boolean(document.fullscreenElement))
        .catch(console.error)
    }
    document.addEventListener('fullscreenchange', syncFullscreen)
    return () => document.removeEventListener('fullscreenchange', syncFullscreen)
  }, [])
  const errorNotice = rpcError && (
    <div
      role="alert"
      className="fixed top-3 right-3 z-[100] flex max-w-md items-center gap-3 rounded-lg border border-destructive bg-card px-4 py-3 text-sm shadow-xl"
    >
      <span>{rpcError}</span>
      <button type="button" className="shrink-0 underline" onClick={clearRpcError}>
        Fechar
      </button>
    </div>
  )

  if (popoutTileParam !== null) {
    return (
      <>
        {errorNotice}
        <PopoutStage
          tileId={popoutTileParam}
          snapshot={snapshot}
          mode={mode}
          wsStats={wsStats}
          onCallRpc={callRpc}
        />
      </>
    )
  }

  if (!snapshot.custom.accountCreated) {
    return (
      <>
        {errorNotice}
        <OnboardingScreen custom={snapshot.custom} onCallRpc={callRpc} />
      </>
    )
  }

  const { custom, room, stream } = snapshot
  const selectedServer =
    custom.savedServers.find(
      (s) => s.serverId.toLowerCase() === custom.selectedServerId.toLowerCase(),
    ) ?? custom.savedServers[0]

  return (
    <div className="app-shell">
      {errorNotice}
      <div className="workspace-topbar">
        <Brand />
        <SpacesNav
          servers={custom.savedServers}
          selectedServerId={selectedServer?.serverId}
          room={room}
          onSelectServer={(serverId) => void callRpc('select_server', { serverId })}
          onOpenCreateServer={() => setServerModal({ open: true, tab: 'create' })}
          onOpenJoinServer={() => setServerModal({ open: true, tab: 'join' })}
        />
        <div className="topbar-actions">
          <button
            type="button"
            className="profile-trigger"
            aria-label="Abrir meu perfil"
            aria-expanded={showProfilePopup}
            onClick={() => setShowProfilePopup((previous) => !previous)}
          >
            <Avatar className="size-8 rounded-xl">
              <AvatarImage src={custom.avatarDataUrl || undefined} alt="" />
              <AvatarFallback className="rounded-xl bg-card text-foreground">
                {custom.displayName.trim()[0]?.toUpperCase() ?? '?'}
              </AvatarFallback>
            </Avatar>
            <span className="max-w-28 truncate">{custom.displayName}</span>
            <ChevronDown className="size-3 text-muted-foreground" />
          </button>
          <Button
            size="icon"
            variant="ghost"
            className="size-8"
            aria-label="Abrir ajustes"
            title="Ajustes"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings className="size-4" />
          </Button>
        </div>
      </div>
      <div className="workspace-body">
        <main className="studio-main">
          <HeaderBar
            snapshot={snapshot}
            focusedTileId={focusedTileId}
            onUnfocus={() => setFocusedTileId(null)}
            onDockAllPopouts={() => void callRpc('set_popout', { clearAll: true })}
            onApplyUpdate={() => void callRpc('apply_update')}
            onOpenSharePicker={() =>
              custom.savedServers.length
                ? setShareOpen(true)
                : setServerModal({ open: true, tab: 'create' })
            }
            onStopSharing={() => void callRpc('stop_sharing')}
          />

          <section className="studio-stage" aria-label="Área de transmissão">
            <StreamStage
              snapshot={snapshot}
              wsStats={wsStats}
              tileOrder={tileOrder}
              focusedTileId={focusedTileId}
              onSetTileOrder={setTileOrder}
              onSetFocusedTileId={setFocusedTileId}
              onCallRpc={callRpc}
              onOpenSharePicker={() => setShareOpen(true)}
              onOpenCreateSpace={() => setServerModal({ open: true, tab: 'create' })}
              onOpenJoinSpace={() => setServerModal({ open: true, tab: 'join' })}
            />

            {showProfilePopup && (
              <div className="profile-popover">
                <Button
                  size="icon"
                  variant="secondary"
                  className="absolute -top-2 -right-2 z-10 size-7 rounded-full"
                  aria-label="Fechar perfil"
                  onClick={() => setShowProfilePopup(false)}
                >
                  <X className="size-3" />
                </Button>
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
        <RoomsPanel
          selectedServer={selectedServer}
          custom={custom}
          room={room}
          stream={stream}
          onCallRpc={callRpc}
          onOpenCreateSpace={() => setServerModal({ open: true, tab: 'create' })}
          onOpenJoinSpace={() => setServerModal({ open: true, tab: 'join' })}
        />
      </div>
      <SessionBar
        snapshot={snapshot}
        mode={mode}
        wsStats={wsStats}
        onUpdateStream={(patch) => void callRpc('update_stream_settings', patch)}
        onOpenInspector={() => setInspectorOpen(true)}
      />

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
