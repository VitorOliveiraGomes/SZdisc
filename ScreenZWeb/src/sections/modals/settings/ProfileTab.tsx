import { Image, LogOut, Trash2 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { hexToRgb, rgbToHex } from '@/lib/color'
import { PRESENCE_OPTIONS, type UserCustomizationDto } from '@/types/bridge'

interface ProfileTabProps {
  custom: UserCustomizationDto
  displayName: string
  userTag: string
  customStatus: string
  bio: string
  customBadgeText: string
  onDraftName: (v: string) => void
  onDraftTag: (v: string) => void
  onDraftStatus: (v: string) => void
  onDraftBio: (v: string) => void
  onDraftBadgeText: (v: string) => void
  onSaveProfile: (patch: Record<string, unknown>) => Promise<void>
  onPickProfileImage: (kind: 'avatar' | 'banner') => Promise<void>
  onClearProfileImage: (kind: 'avatar' | 'banner') => Promise<void>
  onLogout: () => Promise<void>
  onCloseDialog: () => void
}

export function ProfileTab({
  custom,
  displayName,
  userTag,
  customStatus,
  bio,
  customBadgeText,
  onDraftName,
  onDraftTag,
  onDraftStatus,
  onDraftBio,
  onDraftBadgeText,
  onSaveProfile,
  onPickProfileImage,
  onClearProfileImage,
  onLogout,
  onCloseDialog,
}: ProfileTabProps) {
  return (
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
            <Input value={displayName} onChange={(e) => onDraftName(e.target.value)} />
          </div>
          <div>
            <label className="mb-1 block text-xs text-muted-foreground">Tag</label>
            <Input value={userTag} onChange={(e) => onDraftTag(e.target.value)} />
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
          <Input value={customStatus} onChange={(e) => onDraftStatus(e.target.value)} />
        </div>

        <div>
          <label className="mb-1 block text-xs text-muted-foreground">Sobre Mim (Bio)</label>
          <Input value={bio} onChange={(e) => onDraftBio(e.target.value)} />
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
          <label className="flex shrink-0 items-center gap-1.5 cursor-pointer">
            <Switch
              checked={custom.customBadgeEnabled}
              onCheckedChange={(v) => void onSaveProfile({ customBadgeEnabled: v })}
            />
            Badge Livre
          </label>
          <Input
            value={customBadgeText}
            onChange={(e) => onDraftBadgeText(e.target.value)}
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
              onCloseDialog()
            }}
          >
            <LogOut className="size-3.5" /> Trocar perfil
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
            Salvar perfil
          </Button>
        </div>
      </div>
    </ScrollArea>
  )
}
