import type { WsStreamStats } from '@/models/types'

interface TileCanvasPipeline {
  canvas: HTMLCanvasElement
  busy: boolean
  latestBuffer: ArrayBuffer | null
  framesInWindow: number
  bytesInWindow: number
}

const BRIDGE_WS_URL = 'ws://127.0.0.1:47839/ws'

/**
 * Cliente WebSocket binário com trava anti-backpressure por `tileId`.
 * Formato recebido do Rust: [tile_id: u32 LE][width: u16 LE][height: u16 LE][jpeg_bytes...]
 */
class VideoStreamClient {
  private ws: WebSocket | null = null
  private reconnectTimer: number | null = null
  private statsTimer: number | null = null
  private pipelines = new Map<number, TileCanvasPipeline>()
  private listeners = new Set<(stats: WsStreamStats) => void>()
  private filterTileId: number | null = null
  private running = false
  private stats: WsStreamStats = {
    connected: false,
    totalFrames: 0,
    fpsByTile: {},
    kbpsByTile: {},
  }

  start(filterTileId: number | null = null): void {
    this.filterTileId = filterTileId
    if (this.running) return
    this.running = true
    this.connect()
    this.statsTimer = window.setInterval(() => {
      const fpsByTile: Record<number, number> = {}
      const kbpsByTile: Record<number, number> = {}
      for (const [tid, pipe] of this.pipelines.entries()) {
        fpsByTile[tid] = pipe.framesInWindow
        kbpsByTile[tid] = Math.round((pipe.bytesInWindow * 8) / 1000)
        pipe.framesInWindow = 0
        pipe.bytesInWindow = 0
      }
      this.stats = {
        ...this.stats,
        fpsByTile,
        kbpsByTile,
      }
      this.emitStats()
    }, 1000)
  }

  stop(): void {
    this.running = false
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.statsTimer !== null) {
      window.clearInterval(this.statsTimer)
      this.statsTimer = null
    }
    if (this.ws) {
      this.ws.close()
      this.ws = null
    }
  }

  registerCanvas(tileId: number, canvas: HTMLCanvasElement | null): void {
    if (!canvas) {
      this.pipelines.delete(tileId)
      return
    }
    const existing = this.pipelines.get(tileId)
    if (existing) {
      existing.canvas = canvas
      return
    }
    this.pipelines.set(tileId, {
      canvas,
      busy: false,
      latestBuffer: null,
      framesInWindow: 0,
      bytesInWindow: 0,
    })
  }

  subscribeStats(cb: (stats: WsStreamStats) => void): () => void {
    this.listeners.add(cb)
    cb(this.stats)
    return () => {
      this.listeners.delete(cb)
    }
  }

  private emitStats(): void {
    for (const cb of this.listeners) {
      cb(this.stats)
    }
  }

  private connect(): void {
    if (!this.running) return
    const url =
      this.filterTileId !== null ? `${BRIDGE_WS_URL}?tileId=${this.filterTileId}` : BRIDGE_WS_URL

    try {
      const ws = new WebSocket(url)
      ws.binaryType = 'arraybuffer'
      this.ws = ws

      ws.onopen = () => {
        this.stats = { ...this.stats, connected: true }
        this.emitStats()
      }

      ws.onmessage = (ev) => {
        if (!(ev.data instanceof ArrayBuffer) || ev.data.byteLength <= 8) return
        const view = new DataView(ev.data)
        const tileId = view.getUint32(0, true)
        const pipe = this.pipelines.get(tileId)
        if (!pipe) return

        pipe.framesInWindow += 1
        pipe.bytesInWindow += ev.data.byteLength
        this.stats.totalFrames += 1

        // Anti-backpressure: descarta intermediário se o canvas ainda estiver decodificando o anterior
        pipe.latestBuffer = ev.data
        if (!pipe.busy) {
          void this.drainTile(pipe)
        }
      }

      ws.onclose = () => {
        if (this.stats.connected) {
          this.stats = { ...this.stats, connected: false }
          this.emitStats()
        }
        this.scheduleReconnect()
      }

      ws.onerror = () => {
        ws.close()
      }
    } catch {
      this.scheduleReconnect()
    }
  }

  private scheduleReconnect(): void {
    if (!this.running || this.reconnectTimer !== null) return
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null
      this.connect()
    }, 1500)
  }

  private async drainTile(pipe: TileCanvasPipeline): Promise<void> {
    pipe.busy = true
    try {
      while (pipe.latestBuffer) {
        const buffer = pipe.latestBuffer
        pipe.latestBuffer = null
        const view = new DataView(buffer)
        const width = view.getUint16(4, true)
        const height = view.getUint16(6, true)

        const blob = new Blob([buffer.slice(8)], { type: 'image/jpeg' })
        const bitmap = await createImageBitmap(blob)
        if (pipe.canvas.width !== width || pipe.canvas.height !== height) {
          pipe.canvas.width = width
          pipe.canvas.height = height
        }
        const ctx2d = pipe.canvas.getContext('2d')
        if (ctx2d) {
          ctx2d.drawImage(bitmap, 0, 0, pipe.canvas.width, pipe.canvas.height)
        }
        bitmap.close()
      }
    } catch {
      // Ignora quadro corrompido ou canvas desmontado
    } finally {
      pipe.busy = false
    }
  }
}

export const videoStreamClient = new VideoStreamClient()
