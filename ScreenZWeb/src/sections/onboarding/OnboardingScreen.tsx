import { useState } from 'react'
import { ArrowUpRight, ImagePlus } from 'lucide-react'

import { Brand } from '@/components/Brand'
import { foregroundForColor } from '@/lib/color'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { rgbToCss } from '@/services/themeSync'
import type { UserCustomizationDto } from '@/types/bridge'

interface OnboardingScreenProps {
  custom: UserCustomizationDto
  onCallRpc: (method: string, params?: Record<string, unknown>) => Promise<void>
}

export function OnboardingScreen({ custom, onCallRpc }: OnboardingScreenProps) {
  const [name, setName] = useState('')
  const [login, setLogin] = useState('')
  const [status, setStatus] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const createProfile = async () => {
    if (!name.trim() || submitting) return
    setSubmitting(true)
    try {
      await onCallRpc('complete_onboarding', {
        displayName: name.trim(),
        usernameLogin: login.trim(),
        customStatus: status.trim(),
      })
    } catch {
      // O aviso global apresenta o erro e permite corrigir o formulário.
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="onboarding">
      <section className="onboarding-story">
        <Brand />
        <div>
          <h1>
            Sua tela,
            <br />
            <em>
              em boa
              <br />
              companhia.
            </em>
          </h1>
          <p>
            Um espaço para compartilhar o jogo, acompanhar uma ideia ou simplesmente estar junto.
          </p>
        </div>
        <div className="eyebrow">ScreenZ · Estúdio de transmissão</div>
      </section>
      <section className="onboarding-form" aria-label="Criar perfil">
        <form
          onSubmit={(event) => {
            event.preventDefault()
            void createProfile()
          }}
        >
          <div className="eyebrow mb-3">Antes de começar</div>
          <h2 className="font-display text-3xl font-semibold tracking-tight">
            Como podemos chamar você?
          </h2>
          <p className="mt-3 text-sm leading-relaxed text-muted-foreground">
            Escolha como você aparece nas salas. Seu perfil é salvo neste dispositivo.
          </p>
          <div className="my-7 flex items-center gap-4">
            <Avatar className="size-16 rounded-2xl">
              <AvatarImage src={custom.avatarDataUrl || undefined} alt="Sua foto de perfil" />
              <AvatarFallback
                className="rounded-2xl text-lg"
                style={{
                  backgroundColor: rgbToCss(custom.avatarBg),
                  color: foregroundForColor(custom.avatarBg),
                }}
              >
                {(name.trim()[0] ?? '?').toUpperCase()}
              </AvatarFallback>
            </Avatar>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void onCallRpc('pick_profile_image', { kind: 'avatar' })}
            >
              <ImagePlus className="size-3.5" />
              Escolher foto
            </Button>
          </div>
          <div className="space-y-4">
            <label className="block space-y-2 text-xs font-medium">
              <span>
                Seu nome <span className="text-primary">*</span>
              </span>
              <Input
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="Como sua turma conhece você"
                autoComplete="nickname"
                required
                maxLength={80}
                className="h-11"
              />
            </label>
            <label className="block space-y-2 text-xs font-medium">
              <span>
                Nome de usuário{' '}
                <span className="font-normal text-muted-foreground">· opcional</span>
              </span>
              <Input
                value={login}
                onChange={(event) => setLogin(event.target.value)}
                placeholder="seu.apelido"
                autoComplete="username"
                className="h-11"
              />
            </label>
            <label className="block space-y-2 text-xs font-medium">
              <span>
                O que está acontecendo?{' '}
                <span className="font-normal text-muted-foreground">· opcional</span>
              </span>
              <Input
                value={status}
                onChange={(event) => setStatus(event.target.value)}
                placeholder="Jogando, criando, por aqui…"
                className="h-11"
              />
            </label>
          </div>
          <Button
            type="submit"
            className="mt-7 h-11 w-full justify-between"
            disabled={!name.trim() || submitting}
          >
            {submitting ? 'Preparando seu espaço…' : 'Entrar no estúdio'}
            <ArrowUpRight className="size-4" />
          </Button>
        </form>
      </section>
    </div>
  )
}
