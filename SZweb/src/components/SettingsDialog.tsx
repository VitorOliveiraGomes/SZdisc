import { useEffect, useState } from 'react'
import { LogOut, Palette, Radio, User } from 'lucide-react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Separator } from '@/components/ui/separator'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  PRESENCE_OPTIONS,
  THEME_PRESETS,
  type RgbTuple,
  type StreamSettingsDto,
  type UserCustomizationDto,
} from '@/models/types'
import { rgbToCss } from '@/services/themeSync'

interface SettingsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  custom: UserCustomizationDto
  stream: StreamSettingsDto
  onSaveProfile: (patch: Record<string, unknown>) => Promise<void>
  onApplyPreset: (presetCode: number) => Promise<void>
  onSaveAppearance: (patch: Record<string, unknown>) => Promise<void>
  onUpdateStream: (patch: Record<string, unknown>) => Promise<void>
  onLogout: () => Promise<void>
}

function rgbToHex(rgb: RgbTuple): string {
  return `#${rgb.map((v) => v.toString(16).padStart(2, '0')).join('')}`
}

function hexToRgb(hex: string): RgbTuple {
  const clean = hex.replace('#', '')
  if (clean.length !== 6) return [88, 101, 242]
  return [
    parseInt(clean.slice(0, 2), 16) || 0,
    parseInt(clean.slice(2, 4), 16) || 0,
    parseInt(clean.slice(4, 6), 16) || 0,
  ]
}

export function SettingsDialog({
  open,
  onOpenChange,
  custom,
  stream,
  onSaveProfile,
  onApplyPreset,
  onSaveAppearance,
  onUpdateStream,
  onLogout,
}: SettingsDialogProps) {
  const [displayName, setDisplayName] = useState(custom.displayName)
  const [userTag, setUserTag] = useState(custom.userTag)
  const [customStatus, setCustomStatus] = useState(custom.customStatus)
  const [bio, setBio] = useState(custom.bio)

  useEffect(() => {
    setDisplayName(custom.displayName)
    setUserTag(custom.userTag)
    setCustomStatus(custom.customStatus)
    setBio(custom.bio)
  }, [custom, open])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <DialogTitle>Personalização do Perfil, Tema shadcn/ui e Transmissão</DialogTitle>
          <DialogDescription>
            Todas as alterações aqui são enviadas via RPC ao backend Rust e aplicadas nas CSS vars do <code>shadcn/ui</code>.
          </DialogDescription>
        </DialogHeader>

        <div className="grid grid-cols-1 gap-5 md:grid-cols-12">
          <div className="md:col-span-7">
            <Tabs defaultValue="profile">
              <TabsList className="w-full">
                <TabsTrigger value="profile">
                  <User className="size-3.5" /> Perfil
                </TabsTrigger>
                <TabsTrigger value="appearance">
                  <Palette className="size-3.5" /> Tema & Cores
                </TabsTrigger>
                <TabsTrigger value="stream">
                  <Radio className="size-3.5" /> Transmissão
                </TabsTrigger>
              </TabsList>

              <TabsContent value="profile" className="mt-3 space-y-3">
                <div className="grid grid-cols-3 gap-2">
                  <div className="col-span-2">
                    <label className="mb-1 block text-xs text-muted-foreground">Nome de Exibição</label>
                    <Input value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
                  </div>
                  <div>
                    <label className="mb-1 block text-xs text-muted-foreground">Tag</label>
                    <Input value={userTag} onChange={(e) => setUserTag(e.target.value)} />
                  </div>
                </div>

                <div>
                  <label className="mb-1 block text-xs text-muted-foreground">Status Personalizado</label>
                  <Input value={customStatus} onChange={(e) => setCustomStatus(e.target.value)} />
                </div>

                <div>
                  <label className="mb-1 block text-xs text-muted-foreground">Bio</label>
                  <Input value={bio} onChange={(e) => setBio(e.target.value)} />
                </div>

                <div>
                  <label className="mb-1.5 block text-xs text-muted-foreground">Presença</label>
                  <div className="flex flex-wrap gap-1.5">
                    {PRESENCE_OPTIONS.map((p) => (
                      <Button
                        key={p.code}
                        size="sm"
                        variant={custom.presenceCode === p.code ? 'default' : 'secondary'}
                        onClick={() => void onSaveProfile({ presenceCode: p.code })}
                      >
                        <span className={`size-2 rounded-full ${p.dotClass}`} />
                        {p.label}
                      </Button>
                    ))}
                  </div>
                </div>

                <div className="flex items-center justify-between pt-2">
                  <Button
                    size="sm"
                    variant="destructive"
                    onClick={() => {
                      void onLogout()
                      onOpenChange(false)
                    }}
                  >
                    <LogOut className="size-3.5" /> Trocar Conta
                  </Button>
                  <Button
                    size="sm"
                    variant="success"
                    onClick={() =>
                      void onSaveProfile({ displayName, userTag, customStatus, bio })
                    }
                  >
                    Salvar Perfil no Rust
                  </Button>
                </div>
              </TabsContent>

              <TabsContent value="appearance" className="mt-3 space-y-3">
                <div>
                  <div className="mb-1.5 text-xs font-semibold text-muted-foreground">
                    TEMAS PREDEFINIDOS
                  </div>
                  <div className="grid grid-cols-2 gap-1.5">
                    {THEME_PRESETS.filter((p) => p.code !== 6).map((preset) => (
                      <Button
                        key={preset.code}
                        size="sm"
                        variant={custom.themePresetCode === preset.code ? 'default' : 'secondary'}
                        className="justify-start text-xs"
                        onClick={() => void onApplyPreset(preset.code)}
                      >
                        {preset.label}
                      </Button>
                    ))}
                  </div>
                </div>

                <Separator />

                <div className="grid grid-cols-2 gap-2.5 text-xs">
                  <label className="flex items-center justify-between rounded-md border border-border bg-secondary/60 px-2.5 py-1.5">
                    <span>Cor Destaque</span>
                    <input
                      type="color"
                      value={rgbToHex(custom.accentRgb)}
                      onChange={(e) => void onSaveAppearance({ accentRgb: hexToRgb(e.target.value) })}
                      className="h-6 w-8 cursor-pointer bg-transparent"
                    />
                  </label>
                  <label className="flex items-center justify-between rounded-md border border-border bg-secondary/60 px-2.5 py-1.5">
                    <span>Barra Lateral</span>
                    <input
                      type="color"
                      value={rgbToHex(custom.bgSidebarRgb)}
                      onChange={(e) => void onSaveAppearance({ bgSidebarRgb: hexToRgb(e.target.value) })}
                      className="h-6 w-8 cursor-pointer bg-transparent"
                    />
                  </label>
                  <label className="flex items-center justify-between rounded-md border border-border bg-secondary/60 px-2.5 py-1.5">
                    <span>Palco Central</span>
                    <input
                      type="color"
                      value={rgbToHex(custom.bgStageRgb)}
                      onChange={(e) => void onSaveAppearance({ bgStageRgb: hexToRgb(e.target.value) })}
                      className="h-6 w-8 cursor-pointer bg-transparent"
                    />
                  </label>
                  <label className="flex items-center justify-between rounded-md border border-border bg-secondary/60 px-2.5 py-1.5">
                    <span>Cards / Painéis</span>
                    <input
                      type="color"
                      value={rgbToHex(custom.bgCardRgb)}
                      onChange={(e) => void onSaveAppearance({ bgCardRgb: hexToRgb(e.target.value) })}
                      className="h-6 w-8 cursor-pointer bg-transparent"
                    />
                  </label>
                </div>

                <div className="space-y-2 pt-1">
                  <div className="flex items-center justify-between text-xs">
                    <span>Arredondamento (--radius)</span>
                    <span className="text-muted-foreground">{custom.cornerRadius.toFixed(0)}px</span>
                  </div>
                  <Slider
                    value={[custom.cornerRadius]}
                    min={0}
                    max={16}
                    step={1}
                    onValueChange={([v]) => void onSaveAppearance({ cornerRadius: v ?? 8 })}
                  />
                </div>
              </TabsContent>

              <TabsContent value="stream" className="mt-3 space-y-3">
                <label className="flex items-center justify-between rounded-lg border border-border bg-secondary/50 p-3 text-xs cursor-pointer">
                  <span>Transmitir áudio isolado (WASAPI Process Loopback)</span>
                  <Switch
                    checked={stream.shareAudio}
                    onCheckedChange={(checked) => void onUpdateStream({ shareAudio: checked })}
                  />
                </label>

                <label className="flex items-center justify-between rounded-lg border border-border bg-secondary/50 p-3 text-xs cursor-pointer">
                  <span>Mostrar cursor do mouse na transmissão</span>
                  <Switch
                    checked={custom.showCursorInStream}
                    onCheckedChange={(checked) => void onUpdateStream({ showCursor: checked })}
                  />
                </label>

                <label className="flex items-center justify-between rounded-lg border border-border bg-secondary/50 p-3 text-xs cursor-pointer">
                  <span>Modo Jogo Inteligente (auto-pausa prévia local)</span>
                  <Switch
                    checked={stream.autoGameEco}
                    onCheckedChange={(checked) => void onUpdateStream({ autoGameEco: checked })}
                  />
                </label>

                <label className="flex items-center justify-between rounded-lg border border-border bg-secondary/50 p-3 text-xs cursor-pointer">
                  <span>Ocultar prévia local sempre</span>
                  <Switch
                    checked={stream.ecoHidePreview}
                    onCheckedChange={(checked) => void onUpdateStream({ ecoHidePreview: checked })}
                  />
                </label>
              </TabsContent>
            </Tabs>
          </div>

          <div className="md:col-span-5 flex flex-col gap-2">
            <div className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              Prévia ao Vivo do Perfil
            </div>
            <div className="overflow-hidden rounded-xl border border-primary/50 bg-[#111214]">
              <div
                className="relative h-20 w-full"
                style={{
                  background: `linear-gradient(90deg, ${rgbToCss(custom.bannerBg)}, ${rgbToCss(custom.bannerBg2)})`,
                }}
              >
                {stream.isSharing && (
                  <Badge variant="live" className="absolute top-2 right-2">
                    AO VIVO
                  </Badge>
                )}
              </div>
              <div className="p-4">
                <div className="flex items-center gap-3">
                  <Avatar
                    className="size-14 border-2"
                    style={{
                      borderColor: rgbToCss(custom.ringColorRgb),
                      backgroundColor: rgbToCss(custom.avatarBg),
                    }}
                  >
                    <AvatarFallback style={{ backgroundColor: rgbToCss(custom.avatarBg) }}>
                      {(displayName.trim()[0] ?? '?').toUpperCase()}
                    </AvatarFallback>
                  </Avatar>
                  <div>
                    <div className="flex items-center gap-1.5">
                      <span className="font-bold" style={{ color: rgbToCss(custom.nameColorRgb) }}>
                        {displayName || 'Novo Usuário'}
                      </span>
                      <span className="text-xs text-muted-foreground">{userTag}</span>
                    </div>
                    <div className="mt-1 flex flex-wrap gap-1">
                      {custom.badgeCrown && <Badge variant="outline">👑 Host</Badge>}
                      {custom.badge1080p && <Badge variant="outline">⚡ 1080p60</Badge>}
                      {custom.badgeRust && <Badge variant="outline">🦀 Rust</Badge>}
                      {custom.badgeGamer && <Badge variant="outline">🎮 Pro</Badge>}
                    </div>
                  </div>
                </div>
                {customStatus && (
                  <p className="mt-3 text-xs text-foreground/90">💬 {customStatus}</p>
                )}
                {bio && (
                  <>
                    <Separator className="my-2.5" />
                    <div className="text-[10px] font-semibold uppercase text-muted-foreground">
                      Sobre Mim
                    </div>
                    <p className="mt-0.5 text-xs text-muted-foreground">{bio}</p>
                  </>
                )}
              </div>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
