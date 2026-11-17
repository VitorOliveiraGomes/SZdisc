import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { PRESENCE_OPTIONS, type UserCustomizationDto } from '@/types/bridge'
import { rgbToCss } from '@/services/themeSync'
import { foregroundForColor } from '@/lib/color'

interface ProfileCardProps {
  custom: UserCustomizationDto
  isSharing: boolean
  overrideDisplayName?: string
  overrideUserTag?: string
  overrideCustomStatus?: string
  overrideBio?: string
}

export function ProfileCard({
  custom,
  isSharing,
  overrideDisplayName,
  overrideUserTag,
  overrideCustomStatus,
  overrideBio,
}: ProfileCardProps) {
  const displayName = overrideDisplayName ?? custom.displayName
  const userTag = overrideUserTag ?? custom.userTag
  const customStatus = overrideCustomStatus ?? custom.customStatus
  const bio = overrideBio ?? custom.bio
  const presenceDot =
    PRESENCE_OPTIONS.find((p) => p.code === custom.presenceCode)?.dotClass ?? 'bg-emerald-500'

  return (
    <div
      className="w-full max-w-sm overflow-hidden border bg-card shadow-xl"
      style={{
        borderRadius: `${Math.max(16, custom.cornerRadius)}px`,
        borderColor: 'var(--input)',
        borderWidth: `${Math.max(1, custom.borderStrokeWidth)}px`,
      }}
    >
      <div
        className="relative h-20 w-full overflow-hidden"
        style={{
          background: `linear-gradient(90deg, ${rgbToCss(custom.bannerBg)}, ${rgbToCss(custom.bannerBg2)})`,
        }}
      >
        {custom.bannerDataUrl && (
          <img src={custom.bannerDataUrl} alt="Banner" className="size-full object-cover" />
        )}
        {isSharing && (
          <Badge variant="live" className="absolute top-2 right-2">
            AO VIVO
          </Badge>
        )}
      </div>

      <div className="p-4">
        <div className="flex items-center gap-3">
          <div className="relative shrink-0">
            <Avatar
              className="size-14 rounded-2xl"
              style={{
                backgroundColor: rgbToCss(custom.avatarBg),
                boxShadow: custom.avatarRingGlow
                  ? `0 0 0 ${custom.ringThickness}px ${rgbToCss(custom.ringColorRgb)}`
                  : undefined,
              }}
            >
              {custom.avatarDataUrl && <AvatarImage src={custom.avatarDataUrl} alt={displayName} />}
              <AvatarFallback
                className="rounded-2xl"
                style={{
                  backgroundColor: rgbToCss(custom.avatarBg),
                  color: foregroundForColor(custom.avatarBg),
                }}
              >
                {(displayName.trim()[0] ?? '?').toUpperCase()}
              </AvatarFallback>
            </Avatar>
            <span
              className={`absolute right-0 bottom-0 size-3.5 rounded-full border-2 border-card ${presenceDot}`}
            />
          </div>

          <div className="min-w-0">
            <div className="flex flex-wrap items-baseline gap-1.5">
              <span
                className="truncate font-display text-xl font-semibold"
                style={{ color: rgbToCss(custom.nameColorRgb) }}
              >
                {displayName || 'Novo Usuário'}
              </span>
              <span className="text-xs text-muted-foreground">{userTag}</span>
            </div>
            <div className="mt-1 flex flex-wrap gap-1">
              {custom.badgeCrown && <Badge variant="outline">Host</Badge>}
              {custom.badge1080p && <Badge variant="outline">1080p60</Badge>}
              {custom.badgeRust && <Badge variant="outline">Rust</Badge>}
              {custom.badgeGamer && <Badge variant="outline">Gamer</Badge>}
              {custom.customBadgeEnabled && custom.customBadgeText.trim() && (
                <Badge
                  variant="outline"
                  style={{
                    borderColor: rgbToCss(custom.customBadgeRgb),
                    color: '#fff',
                  }}
                >
                  {custom.customBadgeText}
                </Badge>
              )}
            </div>
          </div>
        </div>

        {customStatus.trim() && (
          <p className="mt-4 text-xs leading-relaxed text-foreground/90">{customStatus}</p>
        )}

        {bio.trim() && (
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
  )
}
