import { useCallback, useEffect, useRef, useState } from 'react'

import type {
  AppSnapshotDto,
  BridgeConnectionMode,
  RpcLogEntry,
  WsStreamStats,
} from '@/types/bridge'
import { backendBridge, createInitialMockSnapshot } from '@/services/backendBridge'
import { applyCustomizationToCssVars } from '@/services/themeSync'
import { videoStreamClient } from '@/services/videoStreamClient'

export function useBridgeState(popoutTileParam: number | null) {
  const [snapshot, setSnapshot] = useState<AppSnapshotDto>(() => createInitialMockSnapshot())
  const [mode, setMode] = useState<BridgeConnectionMode>('mock')
  const [logs, setLogs] = useState<RpcLogEntry[]>([])
  const [rpcError, setRpcError] = useState('')
  const [wsStats, setWsStats] = useState<WsStreamStats>({
    connected: false,
    totalFrames: 0,
    fpsByTile: {},
    kbpsByTile: {},
  })
  const inFlightRef = useRef(false)
  const rpcQueueRef = useRef<Promise<void>>(Promise.resolve())

  const syncFromRust = useCallback(async () => {
    if (inFlightRef.current) return
    inFlightRef.current = true
    try {
      const next = await backendBridge.fetchState()
      setSnapshot(next)
      setMode(backendBridge.getMode())
      applyCustomizationToCssVars(next.custom)
    } finally {
      inFlightRef.current = false
    }
  }, [])

  const callRpc = useCallback((method: string, params: Record<string, unknown> = {}) => {
    const request = rpcQueueRef.current.then(async () => {
      setRpcError('')
      const next = await backendBridge.invoke<AppSnapshotDto>(method, params)
      setSnapshot(next)
      setMode(backendBridge.getMode())
      applyCustomizationToCssVars(next.custom)
    })
    rpcQueueRef.current = request.catch((error: unknown) => {
      setRpcError(error instanceof Error ? error.message : String(error))
    })
    return request
  }, [])

  useEffect(() => {
    videoStreamClient.start(popoutTileParam)
    const unsubWs = videoStreamClient.subscribeStats(setWsStats)
    const unsubLogs = backendBridge.subscribeLogs((entries, connMode) => {
      setLogs(entries)
      setMode(connMode)
    })

    const kickoff = window.setTimeout(() => {
      void syncFromRust()
    }, 0)
    const pollId = window.setInterval(() => {
      void syncFromRust()
    }, 500)

    return () => {
      window.clearTimeout(kickoff)
      window.clearInterval(pollId)
      unsubWs()
      unsubLogs()
      videoStreamClient.stop()
    }
  }, [popoutTileParam, syncFromRust])

  return {
    snapshot,
    mode,
    logs,
    wsStats,
    syncFromRust,
    callRpc,
    rpcError,
    clearRpcError: () => setRpcError(''),
  }
}
