import type { RgbTuple } from '@/types/bridge'

export function foregroundForColor(rgb: RgbTuple): string {
  const linear = rgb.map((channel) => {
    const value = channel / 255
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  })
  const luminance = linear[0]! * 0.2126 + linear[1]! * 0.7152 + linear[2]! * 0.0722
  return luminance > 0.179 ? '#000000' : '#ffffff'
}

export function rgbToHex(rgb: RgbTuple): string {
  return `#${rgb.map((v) => v.toString(16).padStart(2, '0')).join('')}`
}

export function hexToRgb(hex: string): RgbTuple {
  const clean = hex.replace('#', '')
  if (clean.length !== 6) return [88, 101, 242]
  return [
    parseInt(clean.slice(0, 2), 16) || 0,
    parseInt(clean.slice(2, 4), 16) || 0,
    parseInt(clean.slice(4, 6), 16) || 0,
  ]
}
