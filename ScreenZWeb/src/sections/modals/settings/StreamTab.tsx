import { Switch } from '@/components/ui/switch'
import type { StreamSettingsDto } from '@/types/bridge'

interface StreamTabProps {
  stream: StreamSettingsDto
  showCursorInStream: boolean
  onUpdateStream: (patch: Record<string, unknown>) => Promise<void>
}

export function StreamTab({ stream, showCursorInStream, onUpdateStream }: StreamTabProps) {
  return (
    <div className="space-y-3">
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
          checked={showCursorInStream}
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

      <label className="flex items-center justify-between rounded-lg border border-border bg-secondary/50 p-3 text-xs cursor-pointer">
        <span>Mostrar minha prévia na grade quando houver outros transmitindo</span>
        <Switch
          checked={stream.showOwnPreviewInGrid}
          onCheckedChange={(checked) => void onUpdateStream({ showOwnPreviewInGrid: checked })}
        />
      </label>
    </div>
  )
}
