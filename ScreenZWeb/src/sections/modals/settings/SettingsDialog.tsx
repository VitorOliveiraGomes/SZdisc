import { useState } from 'react'
import { Palette, Radio, User } from 'lucide-react'

import { ProfileCard } from '@/components/ProfileCard'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import type { StreamSettingsDto, UserCustomizationDto } from '@/types/bridge'

import { AppearanceTab } from './AppearanceTab'
import { ProfileTab } from './ProfileTab'
import { StreamTab } from './StreamTab'

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
          <DialogTitle>Seu ScreenZ</DialogTitle>
          <DialogDescription>Perfil, aparência e preferências de transmissão.</DialogDescription>
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
                <ProfileTab
                  custom={custom}
                  displayName={displayName}
                  userTag={userTag}
                  customStatus={customStatus}
                  bio={bio}
                  customBadgeText={customBadgeText}
                  onDraftName={setDraftName}
                  onDraftTag={setDraftTag}
                  onDraftStatus={setDraftStatus}
                  onDraftBio={setDraftBio}
                  onDraftBadgeText={setDraftBadgeText}
                  onSaveProfile={onSaveProfile}
                  onPickProfileImage={onPickProfileImage}
                  onClearProfileImage={onClearProfileImage}
                  onLogout={onLogout}
                  onCloseDialog={() => handleOpenChange(false)}
                />
              </TabsContent>

              <TabsContent value="appearance" className="mt-3">
                <AppearanceTab
                  custom={custom}
                  onApplyPreset={onApplyPreset}
                  onSaveAppearance={onSaveAppearance}
                />
              </TabsContent>

              <TabsContent value="stream" className="mt-3">
                <StreamTab
                  stream={stream}
                  showCursorInStream={custom.showCursorInStream}
                  onUpdateStream={onUpdateStream}
                />
              </TabsContent>
            </Tabs>
          </div>

          <div className="flex flex-col gap-2 md:col-span-5">
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
