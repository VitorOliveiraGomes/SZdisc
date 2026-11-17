import type { WsStreamStats } from '@/types/bridge'

interface TileCanvasPipeline {
  canvas: HTMLCanvasElement
  ctx: CanvasRenderingContext2D | null
  busy: boolean
  latestBuffer: ArrayBuffer | null
  framesInWindow: number
  bytesInWindow: number
}

const BRIDGE_WS_URL = 'ws://127.0.0.1:47839/ws'

function acquireContext(canvas: HTMLCanvasElement): CanvasRenderingContext2D | null {
  return canvas.getContext('2d', { alpha: false, desynchronized: true })
}

/**
 * Cliente WebSocket binário com trava anti-backpressure por `tileId`.
 * Formato recebido do Rust: `[tile_id: u32 LE][width: u16 LE][height: u16 LE][jpeg_bytes...]`
 */
class VideoStreamClient {
  private ws: WebSocket | null = null
  private reconnectTimer: number | null = null
  private statsTimer: number | null = null
  private pipelines = new Map<number, TileCanvasPipeline>()
  private listeners = new Set<(stats: WsStreamStats) => void>()
  private filterTileId: number | null = null
  private running = false
  private sessionId = 0
  private totalFramesReceived = 0
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
      let changed = this.stats.totalFrames !== this.totalFramesReceived

      for (const [tid, pipe] of this.pipelines.entries()) {
        fpsByTile[tid] = pipe.framesInWindow
        kbpsByTile[tid] = Math.round((pipe.bytesInWindow * 8) / 1000)
        if (
          this.stats.fpsByTile[tid] !== fpsByTile[tid] ||
          this.stats.kbpsByTile[tid] !== kbpsByTile[tid]
        ) {
          changed = true
        }
        pipe.framesInWindow = 0
        pipe.bytesInWindow = 0
      }

      if (Object.keys(this.stats.fpsByTile).length !== this.pipelines.size) changed = true
      if (!changed) return

      this.stats = {
        ...this.stats,
        totalFrames: this.totalFramesReceived,
        fpsByTile,
        kbpsByTile,
      }
      this.emitStats()
    }, 1000)
  }

  stop(): void {
    this.running = false
    this.sessionId += 1
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.statsTimer !== null) {
      window.clearInterval(this.statsTimer)
      this.statsTimer = null
    }
    if (this.ws) {
      const ws = this.ws
      this.ws = null
      ws.close()
    }
    for (const pipe of this.pipelines.values()) {
      pipe.latestBuffer = null
      pipe.framesInWindow = 0
      pipe.bytesInWindow = 0
      pipe.ctx?.clearRect(0, 0, pipe.canvas.width, pipe.canvas.height)
      delete pipe.canvas.dataset.frameReady
    }
    this.stats = { ...this.stats, connected: false, fpsByTile: {}, kbpsByTile: {} }
    this.emitStats()
  }

  registerCanvas(tileId: number, canvas: HTMLCanvasElement | null): void {
    if (!canvas) {
      const prev = this.pipelines.get(tileId)
      if (prev) prev.latestBuffer = null
      prev?.ctx?.clearRect(0, 0, prev.canvas.width, prev.canvas.height)
      if (prev) delete prev.canvas.dataset.frameReady
      this.pipelines.delete(tileId)
      return
    }
    const existing = this.pipelines.get(tileId)
    if (existing) {
      if (existing.canvas !== canvas) {
        existing.canvas = canvas
        existing.ctx = acquireContext(canvas)
      }
      return
    }
    this.pipelines.set(tileId, {
      canvas,
      ctx: acquireContext(canvas),
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
        if (this.ws !== ws || !this.running) return
        this.stats = { ...this.stats, connected: true }
        this.emitStats()
      }

      ws.onmessage = (ev) => {
        if (this.ws !== ws || !this.running) return
        if (!(ev.data instanceof ArrayBuffer) || ev.data.byteLength <= 8) return
        const view = new DataView(ev.data)
        const tileId = view.getUint32(0, true)
        this.totalFramesReceived += 1

        const pipe = this.pipelines.get(tileId)
        if (!pipe) return

        pipe.framesInWindow += 1
        pipe.bytesInWindow += ev.data.byteLength
        pipe.latestBuffer = ev.data
        if (!pipe.busy) {
          void this.drainTile(tileId, pipe)
        }
      }

      ws.onclose = () => {
        if (this.ws !== ws) return
        this.ws = null
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

  private async drainTile(tileId: number, pipe: TileCanvasPipeline): Promise<void> {
    pipe.busy = true
    const sessionId = this.sessionId
    try {
      while (pipe.latestBuffer && this.running && this.pipelines.get(tileId) === pipe) {
        const buffer = pipe.latestBuffer
        pipe.latestBuffer = null
        const view = new DataView(buffer)
        const width = view.getUint16(4, true)
        const height = view.getUint16(6, true)
        if (width === 0 || height === 0) continue

        try {
          const blob = new Blob([new Uint8Array(buffer, 8)], { type: 'image/jpeg' })
          const bitmap = await createImageBitmap(blob)
          try {
            if (
              sessionId !== this.sessionId ||
              !this.running ||
              this.pipelines.get(tileId) !== pipe
            ) {
              return
            }
            if (pipe.canvas.width !== width || pipe.canvas.height !== height) {
              pipe.canvas.width = width
              pipe.canvas.height = height
            }
            pipe.ctx?.drawImage(bitmap, 0, 0, pipe.canvas.width, pipe.canvas.height)
            pipe.canvas.dataset.frameReady = 'true'
          } finally {
            bitmap.close()
          }
        } catch (err) {
          if (import.meta.env.DEV) {
            console.debug('[VideoStreamClient] quadro descartado:', err)
          }
        }
      }
    } finally {
      pipe.busy = false
      if (pipe.latestBuffer && this.running && this.pipelines.get(tileId) === pipe) {
        void this.drainTile(tileId, pipe)
      }
    }
  }
}

export const videoStreamClient = new VideoStreamClient()
