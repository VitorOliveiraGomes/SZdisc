export type RgbTuple = [number, number, number]

export type ResolutionPreset = '1080p' | '900p' | '720p' | '480p' | '360p'

export type SourceCategory = 'Application' | 'Browser' | 'Screen'

export type BridgeConnectionMode = 'rust' | 'mock' | 'offline'

export interface ServerProfileDto {
  serverId: string
  name: string
  iconRgb: RgbTuple
  rooms: string[]
  inviteToken: string
}

export interface UserCustomizationDto {
  accountCreated: boolean
  usernameLogin: string
  displayName: string
  userTag: string
  customStatus: string
  bio: string
  presenceCode: number
  avatarDataUrl: string
  bannerDataUrl: string
  avatarBg: RgbTuple
  bannerBg: RgbTuple
  bannerBg2: RgbTuple
  nameColorRgb: RgbTuple
  ringColorRgb: RgbTuple
  avatarRingGlow: boolean
  ringThickness: number
  badge1080p: boolean
  badgeRust: boolean
  badgeGamer: boolean
  badgeCrown: boolean
  customBadgeEnabled: boolean
  customBadgeText: string
  customBadgeRgb: RgbTuple
  themePresetCode: number
  accentRgb: RgbTuple
  bgSidebarRgb: RgbTuple
  bgStageRgb: RgbTuple
  bgCardRgb: RgbTuple
  textRgb: RgbTuple
  cornerRadius: number
  borderStrokeWidth: number
  uiScale: number
  showCursorInStream: boolean
  selectedServerId: string
  savedServers: ServerProfileDto[]
}

export interface PeerInfoDto {
  peerId: number
  name: string
  isSharing: boolean
  resLabel: string
  videoSize: [number, number]
  fps: number
  mbps: number
  volumePercent: number
  muted: boolean
}

export interface ServerMemberPresenceDto {
  peerId: number
  name: string
  serverId: string
  currentRoom: string
  isSharing: boolean
  resLabel: string
}

export interface RoomSnapshotDto {
  inRoom: boolean
  connectedServerId: string
  activeRoomName: string
  peers: PeerInfoDto[]
  serverMembers: ServerMemberPresenceDto[]
}

export interface StreamSettingsDto {
  isSharing: boolean
  resolution: ResolutionPreset
  fps: 15 | 30 | 60
  jpegQuality: number
  shareAudio: boolean
  ecoHidePreview: boolean
  autoGameEco: boolean
  showOwnPreviewInGrid: boolean
  activeSourceId: string
}

export interface TelemetryDto {
  ramMb: number
  localFps: number
  localMbps: number
  localVideoSize: [number, number]
  wsClients: number
}

export interface AppSnapshotDto {
  version: string
  bridgePort: number
  custom: UserCustomizationDto
  room: RoomSnapshotDto
  stream: StreamSettingsDto
  telemetry: TelemetryDto
  popoutTiles: number[]
  updateStatus: string
  activeToast: string
}

export interface CaptureSourceDto {
  id: string
  name: string
  subtitle: string
  exeName: string
  pid: number
  category: SourceCategory
}

export interface RpcLogEntry {
  id: number
  timestamp: string
  method: string
  mode: BridgeConnectionMode
  ok: boolean
  latencyMs: number
  detail: string
}

export interface WsStreamStats {
  connected: boolean
  totalFrames: number
  fpsByTile: Record<number, number>
  kbpsByTile: Record<number, number>
}

export const THEME_PRESETS: ReadonlyArray<{ code: number; label: string }> = [
  { code: 0, label: 'SZ Original · Grafite e lima' },
  { code: 1, label: 'Meia-Noite OLED (Preto Puro)' },
  { code: 2, label: 'Cyberpunk Neon (Roxo/Rosa)' },
  { code: 3, label: 'Esmeralda Gamer (Verde)' },
  { code: 4, label: 'Rubi Carmesim (Vermelho)' },
  { code: 5, label: 'Oceano Ártico (Azul)' },
  { code: 6, label: 'Personalizado (Livre)' },
]

export const PRESENCE_OPTIONS: ReadonlyArray<{ code: number; label: string; dotClass: string }> = [
  { code: 0, label: 'Online', dotClass: 'bg-emerald-500' },
  { code: 1, label: 'Ausente', dotClass: 'bg-amber-400' },
  { code: 2, label: 'Não Perturbe', dotClass: 'bg-red-500' },
  { code: 3, label: 'Invisível', dotClass: 'bg-zinc-500' },
]
