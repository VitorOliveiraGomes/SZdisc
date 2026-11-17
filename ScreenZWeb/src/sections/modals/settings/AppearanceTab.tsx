import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { Slider } from '@/components/ui/slider'
import { hexToRgb, rgbToHex } from '@/lib/color'
import { THEME_PRESETS, type UserCustomizationDto } from '@/types/bridge'

interface AppearanceTabProps {
  custom: UserCustomizationDto
  onApplyPreset: (presetCode: number) => Promise<void>
  onSaveAppearance: (patch: Record<string, unknown>) => Promise<void>
}

export function AppearanceTab({ custom, onApplyPreset, onSaveAppearance }: AppearanceTabProps) {
  return (
    <div className="space-y-3">
      <div>
        <div className="mb-1.5 text-xs font-semibold text-muted-foreground">TEMAS PREDEFINIDOS</div>
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
          <span>Cantos arredondados</span>
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
    </div>
  )
}
