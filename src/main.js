const { app, BrowserWindow, ipcMain, desktopCapturer, session, screen } = require('electron');
const path = require('path');
const os = require('os');
const dgram = require('dgram');
const http = require('http');
const { WebSocketServer } = require('ws');

// Flags de aceleração de hardware e baixa latência para 1080p 60fps leve
app.commandLine.appendSwitch('enable-gpu-rasterization');
app.commandLine.appendSwitch('enable-zero-copy');
app.commandLine.appendSwitch('disable-renderer-backgrounding');
app.commandLine.appendSwitch('disable-background-timer-throttling');
app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required');
app.commandLine.appendSwitch(
  'enable-features',
  'PlatformHEVCEncoderSupport,CanvasOopRasterization,WebRTC-H264WithOpenH264FFmpeg'
);

let mainWindow = null;
let selectedCaptureConfig = {
  sourceId: null,
  captureAudio: true
};

// Descoberta LAN (UDP) + Servidor WebSocket Local para salas sem precisar de navegador
const LAN_DISCOVERY_PORT = 47831;
let udpSocket = null;
let lanAnnounceInterval = null;
let hostedRoomInfo = null;
let wss = null;
let httpServer = null;
let wsClients = new Map(); // peerId -> ws

function getLocalIPs() {
  const interfaces = os.networkInterfaces();
  const ips = [];
  for (const name of Object.keys(interfaces)) {
    for (const iface of interfaces[name]) {
      if (iface.family === 'IPv4' && !iface.internal) {
        ips.push(iface.address);
      }
    }
  }
  return ips.length > 0 ? ips : ['127.0.0.1'];
}

function initLanDiscovery() {
  try {
    udpSocket = dgram.createSocket({ type: 'udp4', reuseAddr: true });

    udpSocket.on('error', (err) => {
      console.warn('UDP Discovery aviso:', err.message);
    });

    udpSocket.on('message', (msg, rinfo) => {
      try {
        const data = JSON.parse(msg.toString());
        if (data && data.type === 'DISC2_ROOM_ANNOUNCE' && mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.webContents.send('lan-room-discovered', {
            roomCode: data.roomCode,
            roomName: data.roomName,
            hostName: data.hostName,
            hostIp: rinfo.address,
            wsPort: data.wsPort,
            hasStream: data.hasStream,
            streamLabel: data.streamLabel,
            memberCount: data.memberCount || 1,
            timestamp: Date.now()
          });
        }
      } catch (_) {}
    });

    udpSocket.bind(LAN_DISCOVERY_PORT, () => {
      try {
        udpSocket.setBroadcast(true);
      } catch (_) {}
    });
  } catch (err) {
    console.warn('Falha ao iniciar descoberta LAN:', err.message);
  }
}

function startLocalSignalingServer(roomMeta) {
  stopLocalSignalingServer();

  return new Promise((resolve) => {
    httpServer = http.createServer();
    wss = new WebSocketServer({ server: httpServer });
    wsClients.clear();

    wss.on('connection', (ws) => {
      let currentPeerId = null;

      ws.on('message', (raw) => {
        try {
          const msg = JSON.parse(raw.toString());

          if (msg.type === 'register') {
            currentPeerId = msg.peerId;
            ws.peerId = msg.peerId;
            ws.userName = msg.userName || 'Usuário';
            wsClients.set(currentPeerId, ws);

            // Envia lista de participantes atuais para quem acabou de entrar
            const peers = [];
            for (const [id, client] of wsClients.entries()) {
              if (id !== currentPeerId && client.readyState === 1) {
                peers.push({ peerId: id, userName: client.userName });
              }
            }
            ws.send(JSON.stringify({
              type: 'room-peers',
              peers,
              roomMeta: hostedRoomInfo
            }));

            // Notifica os demais que um novo participante entrou
            broadcastToLocalRoom({
              type: 'peer-joined',
              peerId: currentPeerId,
              userName: ws.userName
            }, currentPeerId);
          } else if (msg.targetPeerId) {
            // Roteia mensagens de sinalização WebRTC (offer, answer, ice-candidate, estado)
            const target = wsClients.get(msg.targetPeerId);
            if (target && target.readyState === 1) {
              target.send(JSON.stringify({
                ...msg,
                fromPeerId: currentPeerId
              }));
            }
          } else if (msg.type === 'broadcast-state') {
            broadcastToLocalRoom({
              ...msg,
              fromPeerId: currentPeerId
            }, currentPeerId);
          }
        } catch (e) {
          console.warn('Erro ao processar mensagem WS:', e.message);
        }
      });

      ws.on('close', () => {
        if (currentPeerId) {
          wsClients.delete(currentPeerId);
          broadcastToLocalRoom({
            type: 'peer-left',
            peerId: currentPeerId
          });
        }
      });
    });

    httpServer.listen(0, '0.0.0.0', () => {
      const address = httpServer.address();
      const port = address ? address.port : 0;
      const ips = getLocalIPs();

      hostedRoomInfo = {
        ...roomMeta,
        wsPort: port,
        ips
      };

      // Anuncia a sala via broadcast UDP na rede local a cada 2s
      if (lanAnnounceInterval) clearInterval(lanAnnounceInterval);
      lanAnnounceInterval = setInterval(() => {
        if (!udpSocket || !hostedRoomInfo) return;
        const payload = Buffer.from(JSON.stringify({
          type: 'DISC2_ROOM_ANNOUNCE',
          roomCode: hostedRoomInfo.roomCode,
          roomName: hostedRoomInfo.roomName,
          hostName: hostedRoomInfo.hostName,
          wsPort: hostedRoomInfo.wsPort,
          hasStream: hostedRoomInfo.hasStream || false,
          streamLabel: hostedRoomInfo.streamLabel || '',
          memberCount: Math.max(1, wsClients.size)
        }));
        try {
          udpSocket.send(payload, 0, payload.length, LAN_DISCOVERY_PORT, '255.255.255.255');
        } catch (_) {}
      }, 2000);

      resolve({ port, ips });
    });

    httpServer.on('error', () => {
      resolve({ port: 0, ips: getLocalIPs() });
    });
  });
}

function broadcastToLocalRoom(obj, excludePeerId = null) {
  const payload = JSON.stringify(obj);
  for (const [id, client] of wsClients.entries()) {
    if (id !== excludePeerId && client.readyState === 1) {
      client.send(payload);
    }
  }
}

function stopLocalSignalingServer() {
  if (lanAnnounceInterval) {
    clearInterval(lanAnnounceInterval);
    lanAnnounceInterval = null;
  }
  hostedRoomInfo = null;
  for (const client of wsClients.values()) {
    try { client.close(); } catch (_) {}
  }
  wsClients.clear();
  if (wss) {
    try { wss.close(); } catch (_) {}
    wss = null;
  }
  if (httpServer) {
    try { httpServer.close(); } catch (_) {}
    httpServer = null;
  }
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1260,
    height: 780,
    minWidth: 940,
    minHeight: 600,
    frame: false,
    backgroundColor: '#111214',
    title: 'PulseCast — Transmissão Leve 1080p 60fps',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: false
    }
  });

  // Handler nativo para captura de tela + áudio loopback do sistema no Windows
  session.defaultSession.setDisplayMediaRequestHandler(async (request, callback) => {
    try {
      const sources = await desktopCapturer.getSources({
        types: ['screen', 'window']
      });
      const chosen =
        sources.find((s) => s.id === selectedCaptureConfig.sourceId) || sources[0];

      if (!chosen) {
        callback({});
        return;
      }

      if (selectedCaptureConfig.captureAudio) {
        callback({ video: chosen, audio: 'loopback' });
      } else {
        callback({ video: chosen });
      }
    } catch (err) {
      console.error('Erro em setDisplayMediaRequestHandler:', err);
      callback({});
    }
  });

  mainWindow.loadFile(path.join(__dirname, 'index.html'));
}

// IPCs de controle de janela (estilo Discord)
ipcMain.on('win-minimize', () => {
  if (mainWindow) mainWindow.minimize();
});

ipcMain.on('win-maximize', () => {
  if (!mainWindow) return;
  if (mainWindow.isMaximized()) {
    mainWindow.unmaximize();
  } else {
    mainWindow.maximize();
  }
});

ipcMain.on('win-close', () => {
  if (mainWindow) mainWindow.close();
});

// Lista telas e janelas abertas com miniaturas em tempo real
ipcMain.handle('get-capture-sources', async () => {
  const sources = await desktopCapturer.getSources({
    types: ['screen', 'window'],
    thumbnailSize: { width: 360, height: 202 },
    fetchWindowIcons: true
  });

  const displays = screen.getAllDisplays();

  return sources.map((source) => {
    const isScreen = source.id.startsWith('screen:');
    let displayBounds = null;
    if (isScreen && source.display_id) {
      const matched = displays.find((d) => String(d.id) === String(source.display_id));
      if (matched) {
        displayBounds = `${matched.size.width}x${matched.size.height}`;
      }
    }
    return {
      id: source.id,
      name: source.name,
      isScreen,
      displayBounds,
      thumbnail: source.thumbnail.toDataURL(),
      appIcon: source.appIcon ? source.appIcon.toDataURL() : null
    };
  });
});

// Define qual tela/janela e se o áudio do Windows será capturado antes de chamar getDisplayMedia
ipcMain.handle('set-capture-target', (_, config) => {
  selectedCaptureConfig = {
    sourceId: config.sourceId,
    captureAudio: Boolean(config.captureAudio)
  };
  return { ok: true };
});

// Hospedagem e atualização de sala local
ipcMain.handle('start-local-room', async (_, roomMeta) => {
  return await startLocalSignalingServer(roomMeta);
});

ipcMain.handle('update-local-room', (_, updates) => {
  if (hostedRoomInfo) {
    hostedRoomInfo = { ...hostedRoomInfo, ...updates };
  }
  return { ok: true };
});

ipcMain.handle('stop-local-room', () => {
  stopLocalSignalingServer();
  return { ok: true };
});

ipcMain.handle('get-system-info', async () => {
  const mem = process.memoryUsage();
  let appMetrics = [];
  try {
    appMetrics = app.getAppMetrics();
  } catch (_) {}
  const totalRamMB = appMetrics.reduce((acc, m) => acc + (m.memory ? m.memory.workingSetSize / 1024 : 0), 0);
  return {
    hostname: os.hostname(),
    ips: getLocalIPs(),
    ramMB: Math.round(totalRamMB || mem.rss / (1024 * 1024))
  };
});

app.whenReady().then(() => {
  initLanDiscovery();
  createWindow();
});

app.on('window-all-closed', () => {
  stopLocalSignalingServer();
  if (udpSocket) {
    try { udpSocket.close(); } catch (_) {}
  }
  app.quit();
});
