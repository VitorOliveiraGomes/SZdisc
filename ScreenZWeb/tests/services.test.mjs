import assert from 'node:assert/strict'
import { after, test } from 'node:test'
import { createServer } from 'vite'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'

const server = await createServer({ server: { middlewareMode: true }, appType: 'custom' })
after(() => server.close())

function deferred() {
  let resolve
  const promise = new Promise((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

test('ponte mantém o estado confirmado pelo Rust', async (t) => {
  const { backendBridge, createInitialMockSnapshot } = await server.ssrLoadModule(
    '/src/services/backendBridge.ts',
  )
  const originalFetch = globalThis.fetch
  t.after(() => {
    globalThis.fetch = originalFetch
  })
  const snapshot = createInitialMockSnapshot()
  const respond = (body, status = 200) => {
    globalThis.fetch = async () => Response.json(body, { status })
  }

  await t.test('preview local funciona antes da primeira conexão', async () => {
    globalThis.fetch = async () => {
      throw new TypeError('Failed to fetch')
    }
    const result = await backendBridge.invoke('save_profile', { displayName: 'Preview' })
    assert.equal(result.custom.displayName, 'Preview')
    assert.equal(backendBridge.getMode(), 'mock')
  })

  await t.test('RPC atualiza o cache usado pelo polling e pelo fallback', async () => {
    respond(snapshot)
    await backendBridge.fetchState()
    const updated = structuredClone(snapshot)
    updated.custom.displayName = 'Perfil salvo'
    respond({ ok: true, data: updated })
    await backendBridge.invoke('save_profile', { displayName: 'Perfil salvo' })

    globalThis.fetch = async () => {
      throw new TypeError('Failed to fetch')
    }
    assert.equal((await backendBridge.fetchState()).custom.displayName, 'Perfil salvo')
    assert.equal(backendBridge.getMode(), 'offline')

    respond(updated)
    assert.equal((await backendBridge.fetchState()).custom.displayName, 'Perfil salvo')
    assert.equal((await backendBridge.fetchState()).custom.displayName, 'Perfil salvo')
    globalThis.fetch = async () => {
      throw new TypeError('Failed to fetch')
    }
    assert.equal((await backendBridge.fetchState()).custom.displayName, 'Perfil salvo')
    assert.equal(backendBridge.getMode(), 'offline')
  })

  await t.test('resposta antiga do polling não sobrescreve um comando', async () => {
    const pendingPoll = deferred()
    globalThis.fetch = () => pendingPoll.promise
    const polling = backendBridge.fetchState()
    const updated = structuredClone(snapshot)
    updated.custom.displayName = 'Estado mais recente'
    respond({ ok: true, data: updated })
    await backendBridge.invoke('save_profile', { displayName: updated.custom.displayName })
    pendingPoll.resolve(Response.json(snapshot))
    assert.equal((await polling).custom.displayName, 'Estado mais recente')
  })

  await t.test('consultas de fontes preservam o snapshot', async () => {
    respond({ ok: true, data: [] })
    assert.deepEqual(await backendBridge.invoke('list_capture_sources'), [])
    globalThis.fetch = async () => {
      throw new TypeError('Failed to fetch')
    }
    assert.equal((await backendBridge.fetchState()).custom.displayName, 'Estado mais recente')
  })

  await t.test('erros do backend e desconexões rejeitam o comando', async () => {
    respond({ ok: false, error: 'Fonte fechada' }, 400)
    await assert.rejects(backendBridge.invoke('start_sharing'), /Fonte fechada/)
    globalThis.fetch = async () => {
      throw new TypeError('Failed to fetch')
    }
    await assert.rejects(
      backendBridge.invoke('save_profile', { displayName: 'Não salvo' }),
      /Failed to fetch/,
    )
    assert.equal((await backendBridge.fetchState()).custom.displayName, 'Estado mais recente')
  })
})

test('Tauri usa IPC para estado e comandos e propaga erros do Rust', async (t) => {
  const { backendBridge, createInitialMockSnapshot } = await server.ssrLoadModule(
    '/src/services/backendBridge.ts',
  )
  const previous = {
    window: globalThis.window,
    isTauri: globalThis.isTauri,
    fetch: globalThis.fetch,
  }
  globalThis.window = {}
  globalThis.isTauri = true
  globalThis.fetch = async () => {
    throw new Error('HTTP não deve ser usado pelo Tauri')
  }
  t.after(() => {
    clearMocks()
    for (const [name, value] of Object.entries(previous)) {
      if (value === undefined) delete globalThis[name]
      else globalThis[name] = value
    }
  })
  const state = createInitialMockSnapshot()
  state.custom.displayName = 'Perfil Tauri'
  const calls = []
  mockIPC((command, args) => {
    calls.push({ command, args })
    if (command === 'bridge_state') return JSON.stringify(state)
    if (args.method === 'save_profile') {
      state.custom.displayName = args.params.displayName
      return structuredClone(state)
    }
    throw 'Comando rejeitado pelo Rust'
  })

  assert.equal((await backendBridge.fetchState()).custom.displayName, 'Perfil Tauri')
  const updated = await backendBridge.invoke('save_profile', { displayName: 'Salvo pelo IPC' })
  assert.equal(updated.custom.displayName, 'Salvo pelo IPC')
  assert.deepEqual(
    calls.map((call) => call.command),
    ['bridge_state', 'bridge_rpc'],
  )
  await assert.rejects(
    backendBridge.invoke('invalid_command'),
    (error) => error === 'Comando rejeitado pelo Rust',
  )
  assert.equal(backendBridge.getMode(), 'rust')
})

test('vídeo descarta conexões e decodificações encerradas', async (t) => {
  const { videoStreamClient } = await server.ssrLoadModule('/src/services/videoStreamClient.ts')
  const originals = {
    window: globalThis.window,
    WebSocket: globalThis.WebSocket,
    createImageBitmap: globalThis.createImageBitmap,
  }
  const sockets = []
  const timers = new Map()
  let nextTimerId = 0
  const addTimer = (callback) => {
    const id = ++nextTimerId
    timers.set(id, callback)
    return id
  }
  globalThis.window = {
    setTimeout: addTimer,
    setInterval: addTimer,
    clearTimeout: (id) => timers.delete(id),
    clearInterval: (id) => timers.delete(id),
  }
  globalThis.WebSocket = class {
    constructor(url) {
      this.url = url
      sockets.push(this)
    }
    close() {}
  }
  t.after(() => {
    videoStreamClient.registerCanvas(7, null)
    videoStreamClient.stop()
    for (const [name, value] of Object.entries(originals)) {
      if (value === undefined) delete globalThis[name]
      else globalThis[name] = value
    }
  })

  const frames = []
  const canvas = {
    dataset: {},
    width: 0,
    height: 0,
    getContext: () => ({ clearRect() {}, drawImage: (bitmap) => frames.push(bitmap) }),
  }
  function sendFrame(socket) {
    const buffer = new ArrayBuffer(9)
    const view = new DataView(buffer)
    view.setUint32(0, 7, true)
    view.setUint16(4, 2, true)
    view.setUint16(6, 2, true)
    socket.onmessage({ data: buffer })
  }

  await t.test('fechamento atrasado do socket anterior preserva a nova conexão', () => {
    videoStreamClient.start()
    const oldSocket = sockets.at(-1)
    oldSocket.onopen()
    videoStreamClient.stop()
    videoStreamClient.start(7)
    const currentSocket = sockets.at(-1)
    currentSocket.onopen()
    let connected
    const unsubscribe = videoStreamClient.subscribeStats((stats) => {
      connected = stats.connected
    })
    oldSocket.onclose()
    assert.equal(connected, true)
    assert.equal(timers.size, 1)
    assert.ok(currentSocket.url.endsWith('?tileId=7'))
    unsubscribe()
  })

  await t.test('canvas removido durante a decodificação recebe zero desenhos', async () => {
    const decoding = deferred()
    globalThis.createImageBitmap = () => decoding.promise
    videoStreamClient.registerCanvas(7, canvas)
    sendFrame(sockets.at(-1))
    videoStreamClient.registerCanvas(7, null)
    let closed = false
    decoding.resolve({
      close: () => {
        closed = true
      },
    })
    await new Promise((resolve) => setImmediate(resolve))
    assert.equal(frames.length, 0)
    assert.equal(closed, true)
  })

  await t.test('parada descarta o bitmap pendente e informa desconexão', async () => {
    const decoding = deferred()
    globalThis.createImageBitmap = () => decoding.promise
    videoStreamClient.registerCanvas(7, canvas)
    sendFrame(sockets.at(-1))
    videoStreamClient.stop()
    let closed = false
    decoding.resolve({
      close: () => {
        closed = true
      },
    })
    await new Promise((resolve) => setImmediate(resolve))
    assert.equal(frames.length, 0)
    assert.equal(closed, true)
    let connected
    const unsubscribe = videoStreamClient.subscribeStats((stats) => {
      connected = stats.connected
    })
    assert.equal(connected, false)
    assert.equal(timers.size, 0)
    unsubscribe()
  })

  await t.test('reinício com decodificação pendente desenha o quadro da nova sessão', async () => {
    const oldDecoding = deferred()
    let oldClosed = false
    let currentClosed = false
    const currentBitmap = {
      close: () => {
        currentClosed = true
      },
    }
    let decodeCount = 0
    globalThis.createImageBitmap = () => {
      decodeCount += 1
      return decodeCount === 1 ? oldDecoding.promise : Promise.resolve(currentBitmap)
    }
    videoStreamClient.start(7)
    sendFrame(sockets.at(-1))
    videoStreamClient.stop()
    videoStreamClient.start(7)
    sendFrame(sockets.at(-1))
    oldDecoding.resolve({
      close: () => {
        oldClosed = true
      },
    })
    await new Promise((resolve) => setImmediate(resolve))
    assert.deepEqual(frames, [currentBitmap])
    assert.equal(canvas.width, 2)
    assert.equal(canvas.height, 2)
    assert.equal(oldClosed, true)
    assert.equal(currentClosed, true)
  })
})
