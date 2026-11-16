import type {
  AppSnapshotDto,
  CaptureSourceDto,
  ResolutionPreset,
  RgbTuple,
  RpcLogEntry,
  ServerProfileDto,
} from '@/models/types'

const BRIDGE_BASE_URL = 'http://127.0.0.1:47839'

export type BridgeConnectionMode = 'rust' | 'mock'

interface RpcEnvelope<T> {
  ok: boolean
  data?: T
  error?: string
}

const PRESET_PALETTES: Record<number, { accent: RgbTuple; sidebar: RgbTuple; stage: RgbTuple; card: RgbTuple; text: RgbTuple }> = {
  0: { accent: [88, 101, 242], sidebar: [30, 31, 34], stage: [17, 18, 20], card: [43, 45, 49], text: [242, 243, 245] },
  1: { accent: [99, 102, 241], sidebar: [10, 10, 13], stage: [4, 4, 6], card: [20, 21, 26], text: [245, 245, 250] },
  2: { accent: [217, 70, 239], sidebar: [22, 15, 38], stage: [13, 9, 24], card: [35, 24, 58], text: [250, 232, 255] },
  3: { accent: [16, 185, 129], sidebar: [15, 28, 23], stage: [9, 17, 14], card: [23, 43, 35], text: [236, 253, 245] },
  4: { accent: [244, 63, 94], sidebar: [29, 15, 19], stage: [18, 9, 12], card: [45, 23, 29], text: [255, 241, 242] },
  5: { accent: [56, 189, 248], sidebar: [15, 25, 40], stage: [10, 16, 28], card: [24, 38, 60], text: [240, 249, 255] },
}

export function createInitialMockSnapshot(): AppSnapshotDto {
  return {
    version: '2.11.0',
    bridgePort: 47839,
    custom: {
      accountCreated: true,
      usernameLogin: 'dev_preview',
      displayName: 'Você (Preview Web)',
      userTag: '#4829',
      customStatus: 'Testando ponte Rust ↔ Vite + shadcn/ui',
      bio: 'Rodando em modo preview local enquanto o executável Rust está fechado.',
      presenceCode: 0,
      avatarDataUrl: '',
      bannerDataUrl: '',
      avatarBg: [88, 101, 242],
      bannerBg: [59, 68, 182],
      bannerBg2: [139, 92, 246],
      nameColorRgb: [255, 255, 255],
      ringColorRgb: [88, 101, 242],
      avatarRingGlow: true,
      ringThickness: 2.2,
      badge1080p: true,
      badgeRust: true,
      badgeGamer: true,
      badgeCrown: false,
      customBadgeEnabled: false,
      customBadgeText: '💎 VIP',
      customBadgeRgb: [236, 72, 153],
      themePresetCode: 0,
      accentRgb: [88, 101, 242],
      bgSidebarRgb: [30, 31, 34],
      bgStageRgb: [17, 18, 20],
      bgCardRgb: [43, 45, 49],
      textRgb: [242, 243, 245],
      cornerRadius: 8,
      borderStrokeWidth: 1,
      uiScale: 1,
      showCursorInStream: true,
      selectedServerId: 'SZ-DEMO1',
      savedServers: [
        {
          serverId: 'SZ-DEMO1',
          name: 'SZdisc Lab',
          iconRgb: [88, 101, 242],
          rooms: ['Sala Geral', 'Valorant', 'Cinema'],
          inviteToken: 'SZ-DEMO1/SZdisc Lab/Sala Geral,Valorant,Cinema',
        },
      ],
    },
    room: {
      inRoom: false,
      connectedServerId: '',
      activeRoomName: '',
      peers: [],
      serverMembers: [],
    },
    stream: {
      isSharing: false,
      resolution: '720p',
      fps: 60,
      jpegQuality: 65,
      shareAudio: true,
      ecoHidePreview: false,
      autoGameEco: true,
      showOwnPreviewInGrid: true,
      activeSourceId: '',
    },
    telemetry: {
      ramMb: 42.0,
      localFps: 0,
      localMbps: 0,
      localVideoSize: [1280, 720],
      wsClients: 0,
    },
    popoutTiles: [],
    updateStatus: 'UpToDate',
    activeToast: '',
  }
}

class BackendBridge {
  private mode: BridgeConnectionMode = 'mock'
  private mockState: AppSnapshotDto = createInitialMockSnapshot()
  /** Último corpo bruto de /api/state, para deduplicar snapshots idênticos sem re-render. */
  private lastStateText = ''
  private logs: RpcLogEntry[] = []
  private nextLogId = 1
  private logListeners = new Set<(logs: RpcLogEntry[], mode: BridgeConnectionMode) => void>()

  getMode(): BridgeConnectionMode {
    return this.mode
  }

  getLogs(): RpcLogEntry[] {
    return this.logs
  }

  subscribeLogs(cb: (logs: RpcLogEntry[], mode: BridgeConnectionMode) => void): () => void {
    this.logListeners.add(cb)
    cb(this.logs, this.mode)
    return () => {
      this.logListeners.delete(cb)
    }
  }

  private pushLog(entry: Omit<RpcLogEntry, 'id' | 'timestamp'>): void {
    const now = new Date().toLocaleTimeString('pt-BR', { hour12: false })
    this.logs = [{ ...entry, id: this.nextLogId++, timestamp: now }, ...this.logs].slice(0, 60)
    for (const cb of this.logListeners) {
      cb(this.logs, this.mode)
    }
  }

  async fetchState(): Promise<AppSnapshotDto> {
    const start = performance.now()
    try {
      const res = await fetch(`${BRIDGE_BASE_URL}/api/state`, {
        method: 'GET',
        signal: AbortSignal.timeout(800),
      })
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      const text = await res.text()
      this.mode = 'rust'
      // Snapshot idêntico ao anterior: devolve o MESMO objeto para o React descartar o re-render
      if (text === this.lastStateText) return this.mockState
      const data = JSON.parse(text) as AppSnapshotDto
      this.lastStateText = text
      this.mockState = data
      return data
    } catch {
      this.lastStateText = ''
      if (this.mode !== 'mock') {
        this.mode = 'mock'
        this.pushLog({
          method: 'get_state',
          mode: 'mock',
          ok: true,
          latencyMs: Math.round(performance.now() - start),
          detail: 'Backend Rust offline (:47839) — usando estado Mock local',
        })
      }
      return this.mockState
    }
  }

  async invoke<T = AppSnapshotDto>(
    method: string,
    params: Record<string, unknown> = {},
  ): Promise<T> {
    const start = performance.now()
    try {
      const res = await fetch(`${BRIDGE_BASE_URL}/api/rpc`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ method, params }),
        signal: AbortSignal.timeout(2500),
      })
      const envelope = (await res.json()) as RpcEnvelope<T>
      const latencyMs = Math.round(performance.now() - start)
      if (!res.ok || !envelope.ok || envelope.data === undefined) {
        const err = envelope.error ?? `HTTP ${res.status}`
        this.mode = 'rust'
        this.pushLog({ method, mode: 'rust', ok: false, latencyMs, detail: err })
        throw new Error(err)
      }
      this.mode = 'rust'
      this.pushLog({ method, mode: 'rust', ok: true, latencyMs, detail: 'OK' })
      return envelope.data
    } catch (err) {
      // Se foi erro semântico retornado pelo próprio Rust, propaga
      if (this.mode === 'rust' && err instanceof Error && !err.message.includes('fetch')) {
        throw err
      }
      // Fallback para Mock local quando o processo Rust não está aberto
      this.mode = 'mock'
      const latencyMs = Math.round(performance.now() - start)
      const data = this.handleMockRpc(method, params) as T
      this.pushLog({
        method,
        mode: 'mock',
        ok: true,
        latencyMs,
        detail: 'Simulado localmente (Rust :47839 offline)',
      })
      return data
    }
  }

  private handleMockRpc(method: string, params: Record<string, unknown>): unknown {
    const s = structuredClone(this.mockState)
    switch (method) {
      case 'get_state':
        return s
      case 'list_capture_sources':
        return [
          {
            id: 'mock-screen-0',
            name: 'Monitor Principal (1920x1080)',
            subtitle: 'Tela Inteira • Mock',
            exeName: 'Display1',
            pid: 0,
            category: 'Screen',
          },
          {
            id: 'mock-app-1',
            name: 'Valorant (Preview Mock)',
            subtitle: 'Aplicativo/Jogo • Mock',
            exeName: 'VALORANT.exe',
            pid: 1234,
            category: 'Application',
          },
          {
            id: 'mock-browser-2',
            name: 'Google Chrome — YouTube',
            subtitle: 'Navegador • Mock',
            exeName: 'chrome.exe',
            pid: 5678,
            category: 'Browser',
          },
        ] satisfies CaptureSourceDto[]
      case 'get_source_thumbnails':
        return {}
      case 'complete_onboarding':
        s.custom.accountCreated = true
        s.custom.displayName = String(params.displayName ?? s.custom.displayName)
        s.custom.usernameLogin = String(params.usernameLogin ?? s.custom.usernameLogin)
        s.custom.customStatus = String(params.customStatus ?? s.custom.customStatus)
        break
      case 'logout':
        s.custom.accountCreated = false
        s.room.inRoom = false
        s.stream.isSharing = false
        break
      case 'save_profile':
        if (params.displayName !== undefined) s.custom.displayName = String(params.displayName)
        if (params.userTag !== undefined) s.custom.userTag = String(params.userTag)
        if (params.customStatus !== undefined) s.custom.customStatus = String(params.customStatus)
        if (params.bio !== undefined) s.custom.bio = String(params.bio)
        if (params.presenceCode !== undefined) s.custom.presenceCode = Number(params.presenceCode)
        if (params.nameColorRgb) s.custom.nameColorRgb = params.nameColorRgb as RgbTuple
        if (params.avatarBg) s.custom.avatarBg = params.avatarBg as RgbTuple
        if (params.bannerBg) s.custom.bannerBg = params.bannerBg as RgbTuple
        if (params.bannerBg2) s.custom.bannerBg2 = params.bannerBg2 as RgbTuple
        if (params.avatarRingGlow !== undefined) s.custom.avatarRingGlow = Boolean(params.avatarRingGlow)
        if (params.ringColorRgb) s.custom.ringColorRgb = params.ringColorRgb as RgbTuple
        if (params.ringThickness !== undefined) s.custom.ringThickness = Number(params.ringThickness)
        if (params.badgeCrown !== undefined) s.custom.badgeCrown = Boolean(params.badgeCrown)
        if (params.badge1080p !== undefined) s.custom.badge1080p = Boolean(params.badge1080p)
        if (params.badgeRust !== undefined) s.custom.badgeRust = Boolean(params.badgeRust)
        if (params.badgeGamer !== undefined) s.custom.badgeGamer = Boolean(params.badgeGamer)
        if (params.customBadgeEnabled !== undefined) {
          s.custom.customBadgeEnabled = Boolean(params.customBadgeEnabled)
        }
        if (params.customBadgeText !== undefined) s.custom.customBadgeText = String(params.customBadgeText)
        if (params.customBadgeRgb) s.custom.customBadgeRgb = params.customBadgeRgb as RgbTuple
        break
      case 'clear_profile_image':
        if (params.kind === 'banner') s.custom.bannerDataUrl = ''
        else s.custom.avatarDataUrl = ''
        break
      case 'set_popout':
        if (params.clearAll) {
          s.popoutTiles = []
        } else if (params.tileId !== undefined) {
          const tid = Number(params.tileId)
          if (params.popout === false) {
            s.popoutTiles = s.popoutTiles.filter((id) => id !== tid)
          } else if (!s.popoutTiles.includes(tid)) {
            s.popoutTiles.push(tid)
          }
        }
        break
      case 'apply_theme_preset': {
        const code = Number(params.presetCode ?? 0)
        const pal = PRESET_PALETTES[code]
        s.custom.themePresetCode = code
        if (pal) {
          s.custom.accentRgb = pal.accent
          s.custom.bgSidebarRgb = pal.sidebar
          s.custom.bgStageRgb = pal.stage
          s.custom.bgCardRgb = pal.card
          s.custom.textRgb = pal.text
        }
        break
      }
      case 'save_appearance':
        if (params.accentRgb) s.custom.accentRgb = params.accentRgb as RgbTuple
        if (params.bgSidebarRgb) s.custom.bgSidebarRgb = params.bgSidebarRgb as RgbTuple
        if (params.bgStageRgb) s.custom.bgStageRgb = params.bgStageRgb as RgbTuple
        if (params.bgCardRgb) s.custom.bgCardRgb = params.bgCardRgb as RgbTuple
        if (params.textRgb) s.custom.textRgb = params.textRgb as RgbTuple
        if (params.cornerRadius !== undefined) s.custom.cornerRadius = Number(params.cornerRadius)
        if (params.uiScale !== undefined) s.custom.uiScale = Number(params.uiScale)
        s.custom.themePresetCode = 6
        break
      case 'create_server': {
        const id = `SZ-${Math.floor(10000 + Math.random() * 89999)}`
        const name = String(params.name ?? 'Novo Servidor')
        const rooms = Array.isArray(params.rooms) && params.rooms.length > 0
          ? (params.rooms as string[])
          : ['Sala Geral']
        const srv: ServerProfileDto = {
          serverId: id,
          name,
          iconRgb: (params.iconRgb as RgbTuple) ?? [88, 101, 242],
          rooms,
          inviteToken: `${id}/${name}/${rooms.join(',')}`,
        }
        s.custom.savedServers.push(srv)
        s.custom.selectedServerId = id
        break
      }
      case 'join_server': {
        const raw = String(params.invite ?? '').trim()
        const [rawId, rawName, rawRooms] = raw.split('/')
        const id = rawId?.toUpperCase().startsWith('SZ-') ? rawId.toUpperCase() : `SZ-${rawId || '99999'}`
        const name = rawName || `Servidor ${id}`
        const rooms = rawRooms ? rawRooms.split(',').map((r) => r.trim()).filter(Boolean) : ['Sala Geral']
        s.custom.savedServers.push({
          serverId: id,
          name,
          iconRgb: [16, 185, 129],
          rooms,
          inviteToken: `${id}/${name}/${rooms.join(',')}`,
        })
        s.custom.selectedServerId = id
        break
      }
      case 'select_server':
        s.custom.selectedServerId = String(params.serverId ?? '')
        break
      case 'remove_server':
        s.custom.savedServers = s.custom.savedServers.filter(
          (srv) => srv.serverId !== String(params.serverId ?? ''),
        )
        s.custom.selectedServerId = s.custom.savedServers[0]?.serverId ?? ''
        break
      case 'create_room': {
        const srv = s.custom.savedServers.find((x) => x.serverId === String(params.serverId))
        const rname = String(params.roomName ?? 'Nova Sala')
        if (srv && !srv.rooms.includes(rname)) srv.rooms.push(rname)
        s.room.inRoom = true
        s.room.connectedServerId = String(params.serverId)
        s.room.activeRoomName = rname
        break
      }
      case 'enter_room':
        s.room.inRoom = true
        s.room.connectedServerId = String(params.serverId)
        s.room.activeRoomName = String(params.roomName)
        break
      case 'leave_room':
        s.room.inRoom = false
        s.room.connectedServerId = ''
        s.room.activeRoomName = ''
        s.stream.isSharing = false
        break
      case 'start_sharing':
        if (!s.room.inRoom && s.custom.savedServers[0]) {
          s.room.inRoom = true
          s.room.connectedServerId = s.custom.savedServers[0].serverId
          s.room.activeRoomName = s.custom.savedServers[0].rooms[0] ?? 'Sala Geral'
        }
        s.stream.isSharing = true
        s.stream.activeSourceId = String(params.sourceId ?? '')
        break
      case 'stop_sharing':
        s.stream.isSharing = false
        break
      case 'update_stream_settings':
        if (params.resolution) s.stream.resolution = params.resolution as ResolutionPreset
        if (params.fps) s.stream.fps = Number(params.fps) as 15 | 30 | 60
        if (params.jpegQuality !== undefined) s.stream.jpegQuality = Number(params.jpegQuality)
        if (params.shareAudio !== undefined) s.stream.shareAudio = Boolean(params.shareAudio)
        if (params.ecoHidePreview !== undefined) s.stream.ecoHidePreview = Boolean(params.ecoHidePreview)
        if (params.autoGameEco !== undefined) s.stream.autoGameEco = Boolean(params.autoGameEco)
        if (params.showCursor !== undefined) s.custom.showCursorInStream = Boolean(params.showCursor)
        if (params.showOwnPreviewInGrid !== undefined) {
          s.stream.showOwnPreviewInGrid = Boolean(params.showOwnPreviewInGrid)
        }
        break
      case 'show_toast':
        s.activeToast = String(params.message ?? 'Toast Mock')
        break
      default:
        break
    }
    this.mockState = s
    this.lastStateText = ''
    return s
  }
}

export const backendBridge = new BackendBridge()
