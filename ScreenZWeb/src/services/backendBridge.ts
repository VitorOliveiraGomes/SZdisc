import type { AppSnapshotDto, BridgeConnectionMode, RpcLogEntry } from '@/types/bridge'
import { createInitialMockSnapshot, simulateMockRpc } from '@/services/mockBridge'
import { invoke as invokeNative, isTauri } from '@tauri-apps/api/core'

const BRIDGE_BASE_URL = 'http://127.0.0.1:47839'

interface RpcEnvelope<T> {
  ok: boolean
  data?: T
  error?: string
}

const RPC_TIMEOUT_MS: Record<string, number> = {
  get_source_thumbnails: 10000,
  start_sharing: 10000,
  pick_profile_image: 60000,
}

class BackendBridge {
  private mode: BridgeConnectionMode = 'mock'
  private state: AppSnapshotDto = createInitialMockSnapshot()
  private lastStateText = ''
  private stateRevision = 0
  private hasConnected = false
  private logs: RpcLogEntry[] = []
  private nextLogId = 1
  private logListeners = new Set<(logs: RpcLogEntry[], mode: BridgeConnectionMode) => void>()

  getMode(): BridgeConnectionMode {
    return this.mode
  }

  subscribeLogs(cb: (logs: RpcLogEntry[], mode: BridgeConnectionMode) => void): () => void {
    this.logListeners.add(cb)
    cb(this.logs, this.mode)
    return () => {
      this.logListeners.delete(cb)
    }
  }

  private pushLog(entry: Omit<RpcLogEntry, 'id' | 'timestamp'>): void {
    const now = new Date().toLocaleTimeString('pt-BR', { hour12: false })
    this.logs = [{ ...entry, id: this.nextLogId++, timestamp: now }, ...this.logs].slice(0, 60)
    for (const cb of this.logListeners) {
      cb(this.logs, this.mode)
    }
  }

  async fetchState(): Promise<AppSnapshotDto> {
    const revision = this.stateRevision
    try {
      let text: string
      if (isTauri()) {
        text = await invokeNative<string>('bridge_state')
      } else {
        const res = await fetch(`${BRIDGE_BASE_URL}/api/state`, {
          method: 'GET',
          signal: AbortSignal.timeout(1200),
        })
        if (!res.ok) throw new Error(`HTTP ${res.status}`)
        text = await res.text()
      }
      if (revision !== this.stateRevision) return this.state
      const data = text === this.lastStateText ? this.state : (JSON.parse(text) as AppSnapshotDto)
      this.mode = 'rust'
      this.hasConnected = true
      this.lastStateText = text
      this.state = data
      return data
    } catch {
      if (revision === this.stateRevision && this.hasConnected) this.mode = 'offline'
      return this.state
    }
  }

  async invoke<T = AppSnapshotDto>(
    method: string,
    params: Record<string, unknown> = {},
  ): Promise<T> {
    const start = performance.now()
    const timeoutMs = RPC_TIMEOUT_MS[method] ?? 3000
    const returnsSnapshot = method !== 'list_capture_sources' && method !== 'get_source_thumbnails'
    if (returnsSnapshot) this.stateRevision += 1
    let receivedResponse = false
    try {
      let envelope: RpcEnvelope<T>
      if (isTauri()) {
        envelope = { ok: true, data: await invokeNative<T>('bridge_rpc', { method, params }) }
      } else {
        const res = await fetch(`${BRIDGE_BASE_URL}/api/rpc`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method, params }),
          signal: AbortSignal.timeout(timeoutMs),
        })
        receivedResponse = true
        this.hasConnected = true
        this.mode = 'rust'
        if (!res.ok) {
          const failure = (await res.json()) as RpcEnvelope<T>
          throw new Error(failure.error ?? `HTTP ${res.status}`)
        }
        envelope = (await res.json()) as RpcEnvelope<T>
      }
      receivedResponse = true
      this.mode = 'rust'
      this.hasConnected = true
      const latencyMs = Math.round(performance.now() - start)
      if (!envelope.ok || envelope.data === undefined) {
        const err = envelope.error ?? 'Resposta RPC inválida'
        throw new Error(err)
      }
      if (returnsSnapshot) {
        this.state = envelope.data as AppSnapshotDto
        this.lastStateText = ''
      }
      this.pushLog({ method, mode: 'rust', ok: true, latencyMs, detail: 'OK' })
      return envelope.data
    } catch (err) {
      const latencyMs = Math.round(performance.now() - start)
      const isTimeout = err instanceof DOMException && err.name === 'TimeoutError'
      if (isTauri() || receivedResponse || this.hasConnected) {
        this.mode = isTauri() || receivedResponse ? 'rust' : 'offline'
        this.pushLog({
          method,
          mode: this.mode,
          ok: false,
          latencyMs,
          detail: isTimeout ? `Timeout após ${timeoutMs}ms` : String(err),
        })
        throw err
      }
      this.mode = 'mock'
      const { nextState, returnValue } = simulateMockRpc(this.state, method, params)
      this.state = nextState
      this.lastStateText = ''
      this.pushLog({
        method,
        mode: 'mock',
        ok: true,
        latencyMs,
        detail: 'Simulado localmente (Rust :47839 offline)',
      })
      return returnValue as T
    } finally {
      if (returnsSnapshot) this.stateRevision += 1
    }
  }
}

export const backendBridge = new BackendBridge()
export { createInitialMockSnapshot }
