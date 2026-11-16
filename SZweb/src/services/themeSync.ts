import type { RgbTuple, UserCustomizationDto } from '@/models/types'

export function rgbToCss(rgb: RgbTuple): string {
  return `rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})`
}

export function rgbShift(rgb: RgbTuple, delta: number): string {
  const clamp = (n: number) => Math.max(0, Math.min(255, Math.round(n + delta)))
  return `rgb(${clamp(rgb[0])}, ${clamp(rgb[1])}, ${clamp(rgb[2])})`
}

/**
 * Converte as cores RGB e geometria do `UserCustomizationDto` (Rust)
 * diretamente para variáveis CSS do `shadcn/ui` em tempo real.
 */
export function applyCustomizationToCssVars(custom: UserCustomizationDto): void {
  const root = document.documentElement
  const accent = rgbToCss(custom.accentRgb)
  const sidebar = rgbToCss(custom.bgSidebarRgb)
  const stage = rgbToCss(custom.bgStageRgb)
  const card = rgbToCss(custom.bgCardRgb)
  const text = rgbToCss(custom.textRgb)

  root.style.setProperty('--background', stage)
  root.style.setProperty('--foreground', text)
  root.style.setProperty('--card', card)
  root.style.setProperty('--card-foreground', text)
  root.style.setProperty('--popover', sidebar)
  root.style.setProperty('--popover-foreground', text)
  root.style.setProperty('--primary', accent)
  root.style.setProperty('--ring', accent)
  root.style.setProperty('--secondary', sidebar)
  root.style.setProperty('--secondary-foreground', text)
  root.style.setProperty('--muted', rgbShift(custom.bgCardRgb, -8))
  root.style.setProperty('--accent', rgbShift(custom.bgCardRgb, 14))
  root.style.setProperty('--accent-foreground', text)
  root.style.setProperty('--sz-sidebar', sidebar)
  root.style.setProperty('--sz-rail', rgbShift(custom.bgSidebarRgb, -11))
  root.style.setProperty('--sz-stage', stage)
  root.style.setProperty('--radius', `${Math.max(4, custom.cornerRadius)}px`)
  root.style.setProperty('--sz-border-width', `${custom.borderStrokeWidth}px`)
}
