import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
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
import { backendBridge } from '@/services/backendBridge'
import type {
  CaptureSourceDto,
  ResolutionPreset,
  SourceCategory,
  StreamSettingsDto,
} from '@/types/bridge'

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
  const [selection, setSelection] = useState<{ id: string; category: SourceCategory }>({
    id: '',
    category: 'Application',
  })
  const { id: selectedId, category: tab } = selection
  const [search, setSearch] = useState('')
  const [loadingThumbs, setLoadingThumbs] = useState(false)
  const [error, setError] = useState('')
  const [starting, setStarting] = useState(false)
  const refreshIdRef = useRef(0)

  const refreshSources = useCallback(async () => {
    const refreshId = ++refreshIdRef.current
    setLoadingThumbs(true)
    setError('')
    try {
      const list = await backendBridge.invoke<CaptureSourceDto[]>('list_capture_sources')
      if (refreshId !== refreshIdRef.current) return
      setSources(list)
      setSelection((previous) => {
        if (list.some((source) => source.id === previous.id)) return previous
        const fallback = list[0]
        return fallback ? { id: fallback.id, category: fallback.category } : { ...previous, id: '' }
      })
      const map = await backendBridge.invoke<Record<string, string>>('get_source_thumbnails')
      if (refreshId === refreshIdRef.current) setThumbs(map)
    } catch (error) {
      if (refreshId === refreshIdRef.current) {
        setError(error instanceof Error ? error.message : String(error))
      }
    } finally {
      if (refreshId === refreshIdRef.current) setLoadingThumbs(false)
    }
  }, [])

  useEffect(() => {
    if (!open) return
    const timer = window.setTimeout(() => {
      void refreshSources()
    }, 0)
    return () => {
      window.clearTimeout(timer)
      refreshIdRef.current += 1
    }
  }, [open, refreshSources])

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
    if (!id || starting) return
    setStarting(true)
    try {
      await onStartSharing(id)
      onOpenChange(false)
    } catch {
      // O aviso global mostra o erro e a seleção permanece disponível.
    } finally {
      setStarting(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <div className="flex items-center justify-between gap-3 pr-6">
            <div>
              <DialogTitle className="flex items-center gap-2">
                <Monitor className="size-5 text-primary" />
                {stream.isSharing ? 'Trocar a tela compartilhada' : 'Compartilhar uma tela'}
              </DialogTitle>
              <DialogDescription>
                Selecione um monitor, navegador ou aplicativo para transmitir em tempo real.
              </DialogDescription>
            </div>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void refreshSources()}
              disabled={loadingThumbs}
            >
              <RefreshCw className={`size-3.5 ${loadingThumbs ? 'animate-spin' : ''}`} />
              Atualizar miniaturas
            </Button>
          </div>
        </DialogHeader>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}

        <div className="flex flex-wrap items-center justify-between gap-2">
          <Tabs
            value={tab}
            onValueChange={(value) =>
              setSelection((prev) => ({ ...prev, category: value as SourceCategory }))
            }
          >
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
                    onClick={() => setSelection((prev) => ({ ...prev, id: src.id }))}
                    onDoubleClick={() => void handleConfirm(src.id)}
                    className={`group flex flex-col overflow-hidden rounded-lg border p-2 text-left transition-all cursor-pointer ${
                      isSelected
                        ? 'border-primary bg-primary/15 ring-2 ring-primary/40'
                        : 'border-border bg-card hover:border-primary/50'
                    }`}
                  >
                    <div className="flex aspect-video w-full items-center justify-center overflow-hidden rounded-md bg-black/70">
                      {thumb ? (
                        <img src={thumb} alt={src.name} className="size-full object-cover" />
                      ) : (
                        <div className="flex flex-col items-center gap-1 text-muted-foreground">
                          <Tv className="size-6 opacity-60" />
                          <span className="text-[11px]">{src.exeName}</span>
                        </div>
                      )}
                    </div>
                    <div className="mt-2 truncate text-xs font-semibold text-foreground">
                      {src.name}
                    </div>
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
            disabled={!selectedId || starting}
            onClick={() => void handleConfirm(selectedId)}
          >
            {stream.isSharing ? 'Trocar tela' : 'Iniciar transmissão'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
