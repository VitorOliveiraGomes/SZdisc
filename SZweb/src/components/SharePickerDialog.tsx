import { useEffect, useMemo, useState } from 'react'
import { Monitor, RefreshCw, Search, Tv, Volume2 } from 'lucide-react'

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
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import type { CaptureSourceDto, ResolutionPreset, SourceCategory, StreamSettingsDto } from '@/models/types'
import { backendBridge } from '@/services/backendBridge'

interface SharePickerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  stream: StreamSettingsDto
  onUpdateStream: (patch: Record<string, unknown>) => Promise<void>
  onStartSharing: (sourceId: string) => Promise<void>
}

export function SharePickerDialog({
  open,
  onOpenChange,
  stream,
  onUpdateStream,
  onStartSharing,
}: SharePickerDialogProps) {
  const [sources, setSources] = useState<CaptureSourceDto[]>([])
  const [thumbs, setThumbs] = useState<Record<string, string>>({})
  const [tab, setTab] = useState<SourceCategory>('Application')
  const [search, setSearch] = useState('')
  const [selectedId, setSelectedId] = useState('')
  const [loadingThumbs, setLoadingThumbs] = useState(false)

  const refreshSources = async () => {
    const list = await backendBridge.invoke<CaptureSourceDto[]>('list_capture_sources')
    setSources(list)
    if (list.length > 0 && !list.some((s) => s.id === selectedId)) {
      const firstInTab = list.find((s) => s.category === tab) ?? list[0]
      if (firstInTab) {
        setSelectedId(firstInTab.id)
        setTab(firstInTab.category)
      }
    }
    setLoadingThumbs(true)
    try {
      const map = await backendBridge.invoke<Record<string, string>>('get_source_thumbnails')
      setThumbs(map)
    } finally {
      setLoadingThumbs(false)
    }
  }

  useEffect(() => {
    if (open) {
      void refreshSources()
    }
  }, [open])

  const counts = useMemo(
    () => ({
      Application: sources.filter((s) => s.category === 'Application').length,
      Browser: sources.filter((s) => s.category === 'Browser').length,
      Screen: sources.filter((s) => s.category === 'Screen').length,
    }),
    [sources],
  )

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    return sources.filter((s) => {
      if (q) {
        return (
          s.name.toLowerCase().includes(q) ||
          s.subtitle.toLowerCase().includes(q) ||
          s.exeName.toLowerCase().includes(q)
        )
      }
      return s.category === tab
    })
  }, [sources, search, tab])

  const handleConfirm = async (id: string) => {
    if (!id) return
    await onStartSharing(id)
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <div className="flex items-center justify-between gap-3 pr-6">
            <div>
              <DialogTitle className="flex items-center gap-2">
                <Monitor className="size-5 text-primary" />
                {stream.isSharing ? 'Trocar Janela Transmitida' : 'Compartilhar Tela na Sala'}
              </DialogTitle>
              <DialogDescription>
                Fontes e miniaturas JPEG reais capturadas pelo módulo <code>capture.rs</code>.
              </DialogDescription>
            </div>
            <Button size="sm" variant="secondary" onClick={() => void refreshSources()} disabled={loadingThumbs}>
              <RefreshCw className={`size-3.5 ${loadingThumbs ? 'animate-spin' : ''}`} />
              Atualizar Miniaturas
            </Button>
          </div>
        </DialogHeader>

        <div className="flex flex-wrap items-center justify-between gap-2">
          <Tabs value={tab} onValueChange={(v) => setTab(v as SourceCategory)}>
            <TabsList>
              <TabsTrigger value="Application">Aplicativos ({counts.Application})</TabsTrigger>
              <TabsTrigger value="Browser">Navegadores ({counts.Browser})</TabsTrigger>
              <TabsTrigger value="Screen">Telas ({counts.Screen})</TabsTrigger>
            </TabsList>
          </Tabs>

          <div className="relative w-56">
            <Search className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground" />
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Filtrar app ou janela..."
              className="pl-8"
            />
          </div>
        </div>

        <ScrollArea className="h-72 rounded-lg border border-border bg-secondary/40 p-3">
          {filtered.length === 0 ? (
            <div className="flex h-60 items-center justify-center text-sm text-muted-foreground">
              Nenhuma fonte encontrada nesta categoria.
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {filtered.map((src) => {
                const isSelected = src.id === selectedId
                const thumb = thumbs[src.id]
                return (
                  <button
                    key={src.id}
                    type="button"
                    onClick={() => setSelectedId(src.id)}
                    onDoubleClick={() => void handleConfirm(src.id)}
                    className={`group flex flex-col overflow-hidden rounded-lg border p-2 text-left transition-all cursor-pointer ${
                      isSelected
                        ? 'border-primary bg-primary/15 ring-2 ring-primary/40'
                        : 'border-border bg-card hover:border-primary/50'
                    }`}
                  >
                    <div className="aspect-video w-full overflow-hidden rounded-md bg-black/70 flex items-center justify-center">
                      {thumb ? (
                        <img src={thumb} alt={src.name} className="size-full object-cover" />
                      ) : (
                        <div className="flex flex-col items-center gap-1 text-muted-foreground">
                          <Tv className="size-6 opacity-60" />
                          <span className="text-[11px]">{src.exeName}</span>
                        </div>
                      )}
                    </div>
                    <div className="mt-2 truncate text-xs font-semibold text-foreground">{src.name}</div>
                    <div className="truncate text-[11px] text-muted-foreground">{src.subtitle}</div>
                  </button>
                )
              })}
            </div>
          )}
        </ScrollArea>

        <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-secondary/70 px-3 py-2.5">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs font-medium text-muted-foreground">Resolução:</span>
            {(['1080p', '900p', '720p', '480p', '360p'] as ResolutionPreset[]).map((preset) => (
              <Button
                key={preset}
                size="sm"
                variant={stream.resolution === preset ? 'default' : 'ghost'}
                className="h-7 px-2 text-xs"
                onClick={() => void onUpdateStream({ resolution: preset })}
              >
                {preset}
              </Button>
            ))}

            <span className="ml-2 text-xs font-medium text-muted-foreground">FPS:</span>
            {([60, 30, 15] as const).map((fps) => (
              <Button
                key={fps}
                size="sm"
                variant={stream.fps === fps ? 'default' : 'ghost'}
                className="h-7 px-2 text-xs"
                onClick={() => void onUpdateStream({ fps })}
              >
                {fps}
              </Button>
            ))}

            <label className="ml-2 flex items-center gap-1.5 text-xs cursor-pointer">
              <Switch
                checked={stream.shareAudio}
                onCheckedChange={(checked) => void onUpdateStream({ shareAudio: checked })}
              />
              <Volume2 className="size-3.5" /> Áudio
            </label>
          </div>

          <Button
            variant="success"
            disabled={!selectedId}
            onClick={() => void handleConfirm(selectedId)}
          >
            {stream.isSharing ? 'Trocar Tela' : 'Ir Ao Vivo'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
