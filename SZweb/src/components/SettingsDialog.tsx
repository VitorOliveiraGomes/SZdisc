import { useState } from 'react'
import { Image, LogOut, Palette, Radio, Trash2, User } from 'lucide-react'

import { ProfileCard } from '@/components/ProfileCard'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
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

interface SettingsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  custom: UserCustomizationDto
  stream: StreamSettingsDto
  onSaveProfile: (patch: Record<string, unknown>) => Promise<void>
  onPickProfileImage: (kind: 'avatar' | 'banner') => Promise<void>
  onClearProfileImage: (kind: 'avatar' | 'banner') => Promise<void>
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
  onPickProfileImage,
  onClearProfileImage,
  onApplyPreset,
  onSaveAppearance,
  onUpdateStream,
  onLogout,
}: SettingsDialogProps) {
  const [draftName, setDraftName] = useState<string | null>(null)
  const [draftTag, setDraftTag] = useState<string | null>(null)
  const [draftStatus, setDraftStatus] = useState<string | null>(null)
  const [draftBio, setDraftBio] = useState<string | null>(null)
  const [draftBadgeText, setDraftBadgeText] = useState<string | null>(null)

  const displayName = draftName ?? custom.displayName
  const userTag = draftTag ?? custom.userTag
  const customStatus = draftStatus ?? custom.customStatus
  const bio = draftBio ?? custom.bio
  const customBadgeText = draftBadgeText ?? custom.customBadgeText

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      setDraftName(null)
      setDraftTag(null)
      setDraftStatus(null)
      setDraftBio(null)
      setDraftBadgeText(null)
    }
    onOpenChange(nextOpen)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <DialogTitle>Personalização do Perfil, Tema shadcn/ui e Transmissão</DialogTitle>
          <DialogDescription>
            Todas as alterações são persistidas pelo backend Rust e aplicadas nas CSS vars do <code>shadcn/ui</code>.
          </DialogDescription>
        </DialogHeader>

        <div className="grid grid-cols-1 gap-5 md:grid-cols-12">
          <div className="md:col-span-7">
            <Tabs defaultValue="profile">
              <TabsList className="w-full">
                <TabsTrigger value="profile">
                  <User className="size-3.5" /> Perfil & Foto
                </TabsTrigger>
                <TabsTrigger value="appearance">
                  <Palette className="size-3.5" /> Tema & Cores
                </TabsTrigger>
                <TabsTrigger value="stream">
                  <Radio className="size-3.5" /> Transmissão
                </TabsTrigger>
              </TabsList>

              <TabsContent value="profile" className="mt-3">
                <ScrollArea className="h-[400px] pr-3">
                  <div className="space-y-3">
                    <div className="flex flex-wrap gap-2">
                      <Button size="sm" variant="secondary" onClick={() => void onPickProfileImage('avatar')}>
                        <Image className="size-3.5" /> Escolher Foto de Perfil...
                      </Button>
                      {custom.avatarDataUrl && (
                        <Button size="sm" variant="ghost" onClick={() => void onClearProfileImage('avatar')}>
                          <Trash2 className="size-3.5" /> Remover Foto
                        </Button>
                      )}
                      <Button size="sm" variant="secondary" onClick={() => void onPickProfileImage('banner')}>
                        <Image className="size-3.5" /> Escolher Banner...
                      </Button>
                      {custom.bannerDataUrl && (
                        <Button size="sm" variant="ghost" onClick={() => void onClearProfileImage('banner')}>
                          <Trash2 className="size-3.5" /> Remover Banner
                        </Button>
                      )}
                    </div>

                    <Separator />

                    <div className="grid grid-cols-4 gap-2">
                      <div className="col-span-2">
                        <label className="mb-1 block text-xs text-muted-foreground">Nome</label>
                        <Input value={displayName} onChange={(e) => setDraftName(e.target.value)} />
                      </div>
                      <div>
                        <label className="mb-1 block text-xs text-muted-foreground">Tag</label>
                        <Input value={userTag} onChange={(e) => setDraftTag(e.target.value)} />
                      </div>
                      <div>
                        <label className="mb-1 block text-xs text-muted-foreground">Cor do Nome</label>
                        <input
                          type="color"
                          value={rgbToHex(custom.nameColorRgb)}
                          onChange={(e) => void onSaveProfile({ nameColorRgb: hexToRgb(e.target.value) })}
                          className="h-9 w-full cursor-pointer rounded-md border border-input bg-secondary px-1"
                        />
                      </div>
                    </div>

                    <div>
                      <label className="mb-1 block text-xs text-muted-foreground">Status Personalizado</label>
                      <Input value={customStatus} onChange={(e) => setDraftStatus(e.target.value)} />
                    </div>

                    <div>
                      <label className="mb-1 block text-xs text-muted-foreground">Sobre Mim (Bio)</label>
                      <Input value={bio} onChange={(e) => setDraftBio(e.target.value)} />
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

                    <div className="grid grid-cols-3 gap-2 text-xs">
                      <label className="flex items-center justify-between rounded-md border border-border bg-secondary/50 px-2 py-1.5">
                        <span>Avatar</span>
                        <input
                          type="color"
                          value={rgbToHex(custom.avatarBg)}
                          onChange={(e) => void onSaveProfile({ avatarBg: hexToRgb(e.target.value) })}
                          className="h-6 w-7 cursor-pointer bg-transparent"
                        />
                      </label>
                      <label className="flex items-center justify-between rounded-md border border-border bg-secondary/50 px-2 py-1.5">
                        <span>Banner 1</span>
                        <input
                          type="color"
                          value={rgbToHex(custom.bannerBg)}
                          onChange={(e) => void onSaveProfile({ bannerBg: hexToRgb(e.target.value) })}
                          className="h-6 w-7 cursor-pointer bg-transparent"
                        />
                      </label>
                      <label className="flex items-center justify-between rounded-md border border-border bg-secondary/50 px-2 py-1.5">
                        <span>Banner 2</span>
                        <input
                          type="color"
                          value={rgbToHex(custom.bannerBg2)}
                          onChange={(e) => void onSaveProfile({ bannerBg2: hexToRgb(e.target.value) })}
                          className="h-6 w-7 cursor-pointer bg-transparent"
                        />
                      </label>
                    </div>

                    <div className="flex items-center gap-3 rounded-md border border-border bg-secondary/40 px-3 py-2 text-xs">
                      <label className="flex items-center gap-2 cursor-pointer">
                        <Switch
                          checked={custom.avatarRingGlow}
                          onCheckedChange={(checked) => void onSaveProfile({ avatarRingGlow: checked })}
                        />
                        Anel Neon
                      </label>
                      <input
                        type="color"
                        value={rgbToHex(custom.ringColorRgb)}
                        onChange={(e) => void onSaveProfile({ ringColorRgb: hexToRgb(e.target.value) })}
                        className="h-6 w-7 cursor-pointer bg-transparent"
                      />
                      <div className="flex-1">
                        <Slider
                          value={[custom.ringThickness]}
                          min={0.5}
                          max={4.5}
                          step={0.1}
                          onValueChange={([v]) => void onSaveProfile({ ringThickness: v ?? 2.2 })}
                        />
                      </div>
                      <span className="w-10 text-right text-muted-foreground">
                        {custom.ringThickness.toFixed(1)}px
                      </span>
                    </div>

                    <div className="flex flex-wrap gap-2 text-xs">
                      <label className="flex items-center gap-1.5 rounded-md border border-border px-2 py-1 cursor-pointer">
                        <Switch
                          checked={custom.badgeCrown}
                          onCheckedChange={(v) => void onSaveProfile({ badgeCrown: v })}
                        />
                        👑 Host
                      </label>
                      <label className="flex items-center gap-1.5 rounded-md border border-border px-2 py-1 cursor-pointer">
                        <Switch
                          checked={custom.badge1080p}
                          onCheckedChange={(v) => void onSaveProfile({ badge1080p: v })}
                        />
                        ⚡ 1080p60
                      </label>
                      <label className="flex items-center gap-1.5 rounded-md border border-border px-2 py-1 cursor-pointer">
                        <Switch
                          checked={custom.badgeRust}
                          onCheckedChange={(v) => void onSaveProfile({ badgeRust: v })}
                        />
                        🦀 Rust
                      </label>
                      <label className="flex items-center gap-1.5 rounded-md border border-border px-2 py-1 cursor-pointer">
                        <Switch
                          checked={custom.badgeGamer}
                          onCheckedChange={(v) => void onSaveProfile({ badgeGamer: v })}
                        />
                        🎮 Pro
                      </label>
                    </div>

                    <div className="flex items-center gap-2 text-xs">
                      <label className="flex items-center gap-1.5 cursor-pointer shrink-0">
                        <Switch
                          checked={custom.customBadgeEnabled}
                          onCheckedChange={(v) => void onSaveProfile({ customBadgeEnabled: v })}
                        />
                        Badge Livre
                      </label>
                      <Input
                        value={customBadgeText}
                        onChange={(e) => setDraftBadgeText(e.target.value)}
                        className="h-8"
                      />
                      <input
                        type="color"
                        value={rgbToHex(custom.customBadgeRgb)}
                        onChange={(e) => void onSaveProfile({ customBadgeRgb: hexToRgb(e.target.value) })}
                        className="h-8 w-9 cursor-pointer bg-transparent"
                      />
                    </div>

                    <div className="flex items-center justify-between pt-2">
                      <Button
                        size="sm"
                        variant="destructive"
                        onClick={() => {
                          void onLogout()
                          handleOpenChange(false)
                        }}
                      >
                        <LogOut className="size-3.5" /> Trocar Conta
                      </Button>
                      <Button
                        size="sm"
                        variant="success"
                        onClick={() =>
                          void onSaveProfile({
                            displayName,
                            userTag,
                            customStatus,
                            bio,
                            customBadgeText,
                          })
                        }
                      >
                        Salvar Perfil no Rust
                      </Button>
                    </div>
                  </div>
                </ScrollArea>
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
                    <span>Texto Principal</span>
                    <input
                      type="color"
                      value={rgbToHex(custom.textRgb)}
                      onChange={(e) => void onSaveAppearance({ textRgb: hexToRgb(e.target.value) })}
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
                    <span>Cantos Arredondados (--radius)</span>
                    <span className="text-muted-foreground">{custom.cornerRadius.toFixed(0)}px</span>
                  </div>
                  <Slider
                    value={[custom.cornerRadius]}
                    min={0}
                    max={16}
                    step={1}
                    onValueChange={([v]) => void onSaveAppearance({ cornerRadius: v ?? 8 })}
                  />
                  <div className="flex items-center justify-between text-xs">
                    <span>Espessura da Borda</span>
                    <span className="text-muted-foreground">{custom.borderStrokeWidth.toFixed(1)}px</span>
                  </div>
                  <Slider
                    value={[custom.borderStrokeWidth]}
                    min={0.5}
                    max={3.0}
                    step={0.1}
                    onValueChange={([v]) => void onSaveAppearance({ borderStrokeWidth: v ?? 1.0 })}
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
            <ProfileCard
              custom={custom}
              isSharing={stream.isSharing}
              overrideDisplayName={displayName}
              overrideUserTag={userTag}
              overrideCustomStatus={customStatus}
              overrideBio={bio}
            />
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
