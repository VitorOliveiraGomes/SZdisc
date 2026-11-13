/* global Peer */

const RESOLUTION_MAP = {
  '1080': { width: 1920, height: 1080, label: '1080p' },
  '720':  { width: 1280, height: 720,  label: '720p' },
  '480':  { width: 854,  height: 480,  label: '480p' },
  '360':  { width: 640,  height: 360,  label: '360p' }
};

const ICE_SERVERS = [
  { urls: 'stun:stun.l.google.com:19302' },
  { urls: 'stun:stun1.l.google.com:19302' },
  { urls: 'stun:stun.cloudflare.com:3478' }
];

const state = {
  myId: 'u-' + Math.random().toString(36).substring(2, 9),
  userName: localStorage.getItem('disc2_username') || 'Jogador',
  inRoom: false,
  isHost: false,
  roomCode: '',
  lanAddress: '',

  // Qualidade selecionada (Padrão: 1080p 60fps)
  resolution: '1080',
  fps: 60,
  bitrateKbps: 6500,
  contentHint: 'motion',
  captureSystemAudio: true,

  // Captura local
  isSharing: false,
  localScreenStream: null,
  selectedSourceId: null,
  selectedSourceName: '',
  sourceTab: 'screen',
  cachedSources: [],
  ecoPreviewPaused: false,

  // Áudio de voz (Microfone / Deafen)
  micEnabled: false,
  deafened: false,
  localMicStream: null,

  // Membros e visualizador ativo
  members: new Map(), // peerId -> { peerId, userName, isSharing, streamLabel, stream }
  watchingPeerId: null, // 'self' ou peerId remoto

  // Conexões P2P (PeerJS para código na internet + WebSocket/RTCPeerConnection para LAN direta)
  peerJs: null,
  peerJsDataConns: new Map(), // peerId -> DataConnection
  peerJsMediaCalls: new Map(), // peerId -> MediaConnection

  wsClient: null,
  lanPeerConns: new Map(), // peerId -> RTCPeerConnection

  // Salas descobertas via UDP na rede local
  lanRooms: new Map(),

  // Telemetria
  showHud: true,
  lastBytesTransferred: 0,
  lastStatsTimestamp: 0
};

// Elementos da UI
const el = {
  btnMinimize: document.getElementById('btnMinimize'),
  btnMaximize: document.getElementById('btnMaximize'),
  btnClose: document.getElementById('btnClose'),
  sysRamBadge: document.getElementById('sysRamBadge'),
  netModeBadge: document.getElementById('netModeBadge'),

  roomSetupPanel: document.getElementById('roomSetupPanel'),
  activeRoomPanel: document.getElementById('activeRoomPanel'),
  btnCreateRoom: document.getElementById('btnCreateRoom'),
  inputJoinCode: document.getElementById('inputJoinCode'),
  btnJoinRoom: document.getElementById('btnJoinRoom'),
  activeRoomTitle: document.getElementById('activeRoomTitle'),
  activeRoomSub: document.getElementById('activeRoomSub'),
  activeRoomLanHint: document.getElementById('activeRoomLanHint'),
  btnCopyRoomCode: document.getElementById('btnCopyRoomCode'),

  lanCountBadge: document.getElementById('lanCountBadge'),
  lanRoomsList: document.getElementById('lanRoomsList'),
  memberCountBadge: document.getElementById('memberCountBadge'),
  membersList: document.getElementById('membersList'),

  userAvatar: document.getElementById('userAvatar'),
  inputUsername: document.getElementById('inputUsername'),
  userStatusText: document.getElementById('userStatusText'),
  btnToggleMicSmall: document.getElementById('btnToggleMicSmall'),
  btnToggleDeafenSmall: document.getElementById('btnToggleDeafenSmall'),

  liveBadge: document.getElementById('liveBadge'),
  stageStreamTitle: document.getElementById('stageStreamTitle'),
  stageStatsBadge: document.getElementById('stageStatsBadge'),
  selectResolution: document.getElementById('selectResolution'),
  selectFps: document.getElementById('selectFps'),
  selectBitrate: document.getElementById('selectBitrate'),
  selectContentHint: document.getElementById('selectContentHint'),

  videoViewport: document.getElementById('videoViewport'),
  emptyStageState: document.getElementById('emptyStageState'),
  ecoPreviewBanner: document.getElementById('ecoPreviewBanner'),
  btnResumeLocalPreview: document.getElementById('btnResumeLocalPreview'),
  streamVideo: document.getElementById('streamVideo'),

  telemetryHud: document.getElementById('telemetryHud'),
  hudResolution: document.getElementById('hudResolution'),
  hudFps: document.getElementById('hudFps'),
  hudBitrate: document.getElementById('hudBitrate'),
  hudPing: document.getElementById('hudPing'),

  playerOverlayControls: document.getElementById('playerOverlayControls'),
  sliderStreamVolume: document.getElementById('sliderStreamVolume'),
  btnToggleEcoPreview: document.getElementById('btnToggleEcoPreview'),
  btnPip: document.getElementById('btnPip'),
  btnFullscreen: document.getElementById('btnFullscreen'),

  connDot: document.getElementById('connDot'),
  connMainText: document.getElementById('connMainText'),
  connSubText: document.getElementById('connSubText'),
  btnShareScreen: document.getElementById('btnShareScreen'),
  btnShareScreenLabel: document.getElementById('btnShareScreenLabel'),
  btnStartShareHero: document.getElementById('btnStartShareHero'),
  btnStopShare: document.getElementById('btnStopShare'),
  btnDockMic: document.getElementById('btnDockMic'),
  btnDockMicLabel: document.getElementById('btnDockMicLabel'),
  btnLeaveRoom: document.getElementById('btnLeaveRoom'),
  btnToggleHud: document.getElementById('btnToggleHud'),

  sourcePickerModal: document.getElementById('sourcePickerModal'),
  btnCloseModal: document.getElementById('btnCloseModal'),
  btnCancelModal: document.getElementById('btnCancelModal'),
  btnConfirmShare: document.getElementById('btnConfirmShare'),
  tabScreens: document.getElementById('tabScreens'),
  tabWindows: document.getElementById('tabWindows'),
  btnRefreshSources: document.getElementById('btnRefreshSources'),
  sourcesGrid: document.getElementById('sourcesGrid'),
  modalResPills: document.getElementById('modalResPills'),
  modalFpsPills: document.getElementById('modalFpsPills'),
  chkCaptureAudio: document.getElementById('chkCaptureAudio'),

  toastContainer: document.getElementById('toastContainer'),
  remoteAudiosContainer: document.getElementById('remoteAudiosContainer')
};

function showToast(message) {
  const t = document.createElement('div');
  t.className = 'toast';
  t.textContent = message;
  el.toastContainer.appendChild(t);
  setTimeout(() => {
    t.remove();
  }, 3200);
}

// Inicialização Geral
async function init() {
  el.inputUsername.value = state.userName;
  updateAvatarText();

  // Controles de janela
  el.btnMinimize.addEventListener('click', () => window.desktopAPI.minimize());
  el.btnMaximize.addEventListener('click', () => window.desktopAPI.maximize());
  el.btnClose.addEventListener('click', () => window.desktopAPI.close());

  // Nome de usuário
  el.inputUsername.addEventListener('change', () => {
    const val = el.inputUsername.value.trim() || 'Jogador';
    state.userName = val;
    el.inputUsername.value = val;
    localStorage.setItem('disc2_username', val);
    updateAvatarText();
    broadcastMyState();
    renderMembersList();
  });

  // Salas
  el.btnCreateRoom.addEventListener('click', () => createRoom());
  el.btnJoinRoom.addEventListener('click', () => {
    const target = el.inputJoinCode.value.trim();
    if (target) joinRoomByInput(target);
  });
  el.inputJoinCode.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      const target = el.inputJoinCode.value.trim();
      if (target) joinRoomByInput(target);
    }
  });
  el.btnCopyRoomCode.addEventListener('click', () => {
    if (!state.roomCode) return;
    navigator.clipboard.writeText(state.roomCode);
    showToast(`Código ${state.roomCode} copiado!`);
  });
  el.btnLeaveRoom.addEventListener('click', () => leaveRoom());

  // Botões de captura de tela
  el.btnShareScreen.addEventListener('click', () => openSourcePickerModal());
  el.btnStartShareHero.addEventListener('click', () => openSourcePickerModal());
  el.btnStopShare.addEventListener('click', () => stopScreenShare());

  // Modal de seleção de fonte
  el.btnCloseModal.addEventListener('click', () => closeSourcePickerModal());
  el.btnCancelModal.addEventListener('click', () => closeSourcePickerModal());
  el.btnRefreshSources.addEventListener('click', () => loadCaptureSources());
  el.btnConfirmShare.addEventListener('click', () => startSelectedScreenShare());

  el.tabScreens.addEventListener('click', () => {
    state.sourceTab = 'screen';
    el.tabScreens.classList.add('active');
    el.tabWindows.classList.remove('active');
    renderSourcesGrid();
  });

  el.tabWindows.addEventListener('click', () => {
    state.sourceTab = 'window';
    el.tabWindows.classList.add('active');
    el.tabScreens.classList.remove('active');
    renderSourcesGrid();
  });

  // Seletores de qualidade dentro do modal
  el.modalResPills.addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-res]');
    if (!btn) return;
    syncQualitySelection(btn.dataset.res, state.fps);
  });

  el.modalFpsPills.addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-fps]');
    if (!btn) return;
    syncQualitySelection(state.resolution, Number(btn.dataset.fps));
  });

  // Seletores rápidos de qualidade na barra superior (alteram em tempo real durante a live!)
  el.selectResolution.addEventListener('change', () => {
    syncQualitySelection(el.selectResolution.value, state.fps);
    applyDynamicStreamConstraints();
  });

  el.selectFps.addEventListener('change', () => {
    syncQualitySelection(state.resolution, Number(el.selectFps.value));
    applyDynamicStreamConstraints();
  });

  el.selectBitrate.addEventListener('change', () => {
    state.bitrateKbps = Number(el.selectBitrate.value);
    applyAllSendersEncodingParams();
    showToast(`Bitrate ajustado para ${(state.bitrateKbps / 1000).toFixed(1)} Mbps`);
  });

  el.selectContentHint.addEventListener('change', () => {
    state.contentHint = el.selectContentHint.value;
    applyDynamicStreamConstraints();
  });

  // Controles do player de vídeo
  el.sliderStreamVolume.addEventListener('input', () => {
    const vol = Number(el.sliderStreamVolume.value) / 100;
    el.streamVideo.volume = vol;
    el.streamVideo.muted = vol === 0 || state.deafened || state.watchingPeerId === 'self';
  });

  el.btnToggleEcoPreview.addEventListener('click', () => toggleEcoPreview());
  el.btnResumeLocalPreview.addEventListener('click', () => toggleEcoPreview(false));

  el.btnPip.addEventListener('click', async () => {
    try {
      if (document.pictureInPictureElement) {
        await document.exitPictureInPicture();
      } else if (el.streamVideo.srcObject) {
        await el.streamVideo.requestPictureInPicture();
      }
    } catch (_) {}
  });

  el.btnFullscreen.addEventListener('click', () => {
    if (!document.fullscreenElement) {
      el.videoViewport.requestFullscreen().catch(() => {});
    } else {
      document.exitFullscreen().catch(() => {});
    }
  });

  // Microfone e Áudio
  el.btnToggleMicSmall.addEventListener('click', () => toggleMicrophone());
  el.btnDockMic.addEventListener('click', () => toggleMicrophone());
  el.btnToggleDeafenSmall.addEventListener('click', () => toggleDeafen());

  // HUD de estatísticas
  el.btnToggleHud.addEventListener('click', () => {
    state.showHud = !state.showHud;
    updateHudVisibility();
  });

  // Recebe salas descobertas na rede local (UDP broadcast)
  window.desktopAPI.onLanRoomDiscovered((room) => {
    if (state.inRoom && state.roomCode === room.roomCode) return;
    state.lanRooms.set(`${room.hostIp}:${room.wsPort}`, room);
    renderLanRooms();
  });

  renderMembersList();
  updateQualityBadgeText();

  // Loop leve de telemetria e uso de RAM (a cada 1.5s)
  setInterval(periodicTelemetryTick, 1500);
  periodicTelemetryTick();
}

function updateAvatarText() {
  el.userAvatar.textContent = (state.userName[0] || 'U').toUpperCase();
}

function syncQualitySelection(resKey, fpsVal) {
  state.resolution = String(resKey);
  state.fps = Number(fpsVal);

  el.selectResolution.value = state.resolution;
  el.selectFps.value = String(state.fps);

  el.modalResPills.querySelectorAll('.pill-btn').forEach((b) => {
    b.classList.toggle('active', b.dataset.res === state.resolution);
  });
  el.modalFpsPills.querySelectorAll('.pill-btn').forEach((b) => {
    b.classList.toggle('active', Number(b.dataset.fps) === state.fps);
  });

  // Ajusta sugestão de bitrate automaticamente caso o usuário baixe ou suba a resolução
  if (state.resolution === '1080' && state.fps === 60 && state.bitrateKbps < 6000) {
    state.bitrateKbps = 6500;
    el.selectBitrate.value = '6500';
  } else if (state.resolution === '720' && state.bitrateKbps > 6000) {
    state.bitrateKbps = 3500;
    el.selectBitrate.value = '3500';
  } else if ((state.resolution === '480' || state.resolution === '360') && state.bitrateKbps > 3500) {
    state.bitrateKbps = 1800;
    el.selectBitrate.value = '1800';
  }

  updateQualityBadgeText();
}

function updateQualityBadgeText() {
  const resInfo = RESOLUTION_MAP[state.resolution] || RESOLUTION_MAP['1080'];
  el.stageStatsBadge.textContent = `${resInfo.label} • ${state.fps} FPS`;
}

// ============================================================================
// CAPTURA DE TELA / JANELA E ALTERAÇÃO EM TEMPO REAL (1080p 60fps -> mais baixo)
// ============================================================================

async function openSourcePickerModal() {
  el.sourcePickerModal.classList.remove('hidden');
  await loadCaptureSources();
}

function closeSourcePickerModal() {
  el.sourcePickerModal.classList.add('hidden');
}

async function loadCaptureSources() {
  el.sourcesGrid.innerHTML = '<div class="empty-hint">Carregando telas e janelas...</div>';
  try {
    const sources = await window.desktopAPI.getCaptureSources();
    state.cachedSources = sources;
    if (!state.selectedSourceId && sources.length > 0) {
      const firstScreen = sources.find((s) => s.isScreen) || sources[0];
      state.selectedSourceId = firstScreen.id;
      state.selectedSourceName = firstScreen.name;
    }
    renderSourcesGrid();
  } catch (err) {
    el.sourcesGrid.innerHTML = `<div class="empty-hint">Erro ao listar fontes: ${err.message}</div>`;
  }
}

function renderSourcesGrid() {
  const wantScreen = state.sourceTab === 'screen';
  const filtered = state.cachedSources.filter((s) => s.isScreen === wantScreen);

  el.sourcesGrid.innerHTML = '';
  if (filtered.length === 0) {
    el.sourcesGrid.innerHTML = '<div class="empty-hint">Nenhuma fonte encontrada nesta categoria.</div>';
    return;
  }

  filtered.forEach((src) => {
    const card = document.createElement('div');
    card.className = 'source-card' + (src.id === state.selectedSourceId ? ' selected' : '');
    card.innerHTML = `
      <img class="source-thumb" src="${src.thumbnail}" alt="${escapeHtml(src.name)}" />
      <div class="source-label-row">
        ${src.appIcon ? `<img class="source-icon" src="${src.appIcon}" alt="" />` : ''}
        <span class="source-name" title="${escapeHtml(src.name)}">${escapeHtml(src.name)}</span>
      </div>
    `;
    card.addEventListener('click', () => {
      state.selectedSourceId = src.id;
      state.selectedSourceName = src.name;
      renderSourcesGrid();
    });
    card.addEventListener('dblclick', () => {
      state.selectedSourceId = src.id;
      state.selectedSourceName = src.name;
      startSelectedScreenShare();
    });
    el.sourcesGrid.appendChild(card);
  });
}

async function startSelectedScreenShare() {
  if (!state.selectedSourceId) {
    showToast('Selecione uma tela ou janela primeiro.');
    return;
  }

  state.captureSystemAudio = el.chkCaptureAudio.checked;
  closeSourcePickerModal();

  // Se ainda não estiver em uma sala, cria uma automaticamente para já poder compartilhar com amigos
  if (!state.inRoom) {
    await createRoom();
  }

  // Encerra captura anterior se existir
  if (state.localScreenStream) {
    state.localScreenStream.getTracks().forEach((t) => t.stop());
    state.localScreenStream = null;
  }

  const res = RESOLUTION_MAP[state.resolution] || RESOLUTION_MAP['1080'];
  const fps = state.fps;

  try {
    await window.desktopAPI.setCaptureTarget({
      sourceId: state.selectedSourceId,
      captureAudio: state.captureSystemAudio
    });

    let stream;
    try {
      stream = await navigator.mediaDevices.getDisplayMedia({
        video: {
          width: { ideal: res.width, max: res.width },
          height: { ideal: res.height, max: res.height },
          frameRate: { ideal: fps, max: fps }
        },
        audio: state.captureSystemAudio
      });
    } catch (_) {
      // Fallback caso o áudio loopback não esteja disponível na fonte escolhida
      stream = await navigator.mediaDevices.getDisplayMedia({
        video: {
          width: { ideal: res.width, max: res.width },
          height: { ideal: res.height, max: res.height },
          frameRate: { ideal: fps, max: fps }
        },
        audio: false
      });
    }

    const videoTrack = stream.getVideoTracks()[0];
    if (videoTrack) {
      videoTrack.contentHint = state.contentHint;
      videoTrack.onended = () => {
        stopScreenShare();
      };
    }

    state.localScreenStream = stream;
    state.isSharing = true;
    state.ecoPreviewPaused = false;

    el.btnShareScreenLabel.textContent = 'Trocar Tela';
    el.btnStopShare.classList.remove('hidden');
    el.userStatusText.textContent = `Transmitindo ${res.label} ${fps}fps`;

    // Mostra no palco
    watchStream('self', stream, `${state.userName} — ${state.selectedSourceName || 'Tela'}`);

    // Envia ou atualiza a transmissão para todos na sala
    publishStreamToAllPeers();
    broadcastMyState();
    renderMembersList();

    if (state.isHost) {
      window.desktopAPI.updateLocalRoom({
        hasStream: true,
        streamLabel: `${res.label} ${fps}fps`
      });
    }

    showToast(`Transmitindo em ${res.label} a ${fps} FPS!`);
  } catch (err) {
    console.error('Erro ao iniciar captura:', err);
    showToast('Não foi possível iniciar a captura da tela.');
  }
}

async function applyDynamicStreamConstraints() {
  const res = RESOLUTION_MAP[state.resolution] || RESOLUTION_MAP['1080'];
  const fps = state.fps;
  updateQualityBadgeText();

  if (!state.localScreenStream || !state.isSharing) return;

  const videoTrack = state.localScreenStream.getVideoTracks()[0];
  if (!videoTrack) return;

  try {
    videoTrack.contentHint = state.contentHint;
    await videoTrack.applyConstraints({
      width: { ideal: res.width, max: res.width },
      height: { ideal: res.height, max: res.height },
      frameRate: { ideal: fps, max: fps }
    });
    await applyAllSendersEncodingParams();
    el.userStatusText.textContent = `Transmitindo ${res.label} ${fps}fps`;
    broadcastMyState();
    showToast(`Qualidade alterada em tempo real: ${res.label} • ${fps} FPS`);
  } catch (err) {
    console.warn('Aviso ao aplicar constraints:', err);
  }
}

function stopScreenShare() {
  if (state.localScreenStream) {
    state.localScreenStream.getTracks().forEach((t) => t.stop());
    state.localScreenStream = null;
  }
  state.isSharing = false;
  state.ecoPreviewPaused = false;

  el.btnShareScreenLabel.textContent = 'Transmitir Tela';
  el.btnStopShare.classList.add('hidden');
  el.userStatusText.textContent = state.inRoom ? `Conectado na Sala #${state.roomCode}` : 'Pronto para transmitir';

  if (state.watchingPeerId === 'self') {
    clearStage();
  }

  // Fecha chamadas de mídia de tela ativas ou avisa os participantes
  for (const call of state.peerJsMediaCalls.values()) {
    try { call.close(); } catch (_) {}
  }
  state.peerJsMediaCalls.clear();

  broadcastMyState();
  renderMembersList();

  if (state.isHost) {
    window.desktopAPI.updateLocalRoom({
      hasStream: false,
      streamLabel: ''
    });
  }

  showToast('Transmissão encerrada.');
}

// ============================================================================
// PALCO DE VÍDEO E MODO JOGO (OCULTAR PRÉVIA LOCAL)
// ============================================================================

function watchStream(peerId, mediaStream, titleText) {
  state.watchingPeerId = peerId;
  el.emptyStageState.classList.add('hidden');
  el.streamVideo.classList.remove('hidden');
  el.playerOverlayControls.classList.remove('hidden');
  el.liveBadge.classList.remove('hidden');
  el.stageStreamTitle.textContent = titleText || 'Transmissão Ao Vivo';

  el.streamVideo.srcObject = mediaStream;
  // Se estiver vendo a própria tela, deixa mudo localmente para não dar eco infinito no áudio do sistema
  el.streamVideo.muted = peerId === 'self' || state.deafened;

  if (peerId === 'self') {
    el.btnToggleEcoPreview.classList.remove('hidden');
    toggleEcoPreview(state.ecoPreviewPaused);
  } else {
    el.btnToggleEcoPreview.classList.add('hidden');
    el.ecoPreviewBanner.classList.add('hidden');
  }

  updateHudVisibility();
  renderMembersList();
}

function clearStage() {
  state.watchingPeerId = null;
  el.streamVideo.srcObject = null;
  el.streamVideo.classList.add('hidden');
  el.playerOverlayControls.classList.add('hidden');
  el.ecoPreviewBanner.classList.add('hidden');
  el.telemetryHud.classList.add('hidden');
  el.liveBadge.classList.add('hidden');
  el.emptyStageState.classList.remove('hidden');
  el.stageStreamTitle.textContent = 'Nenhuma transmissão ativa';
  renderMembersList();
}

function toggleEcoPreview(forceState) {
  if (state.watchingPeerId !== 'self') return;
  state.ecoPreviewPaused = typeof forceState === 'boolean' ? forceState : !state.ecoPreviewPaused;

  if (state.ecoPreviewPaused) {
    el.streamVideo.pause();
    el.ecoPreviewBanner.classList.remove('hidden');
    el.btnToggleEcoPreview.textContent = 'Mostrar Prévia';
  } else {
    el.streamVideo.play().catch(() => {});
    el.ecoPreviewBanner.classList.add('hidden');
    el.btnToggleEcoPreview.textContent = 'Modo Jogo (Ocultar Prévia)';
  }
}

function updateHudVisibility() {
  if (state.watchingPeerId && state.showHud) {
    el.telemetryHud.classList.remove('hidden');
  } else {
    el.telemetryHud.classList.add('hidden');
  }
}

// ============================================================================
// SALAS APP-PARA-APP (CÓDIGO P2P INTERNET + SERVIDOR LOCAL LAN/VPN)
// ============================================================================

async function createRoom() {
  leaveRoom(true);

  const code = String(Math.floor(1000 + Math.random() * 9000));
  state.roomCode = code;
  state.isHost = true;
  state.inRoom = true;

  // 1. Inicia servidor local de sinalização (funciona em LAN / Radmin / ZeroTier mesmo sem internet)
  const localInfo = await window.desktopAPI.startLocalRoom({
    roomCode: code,
    roomName: `Sala de ${state.userName}`,
    hostName: state.userName,
    hasStream: state.isSharing,
    streamLabel: `${RESOLUTION_MAP[state.resolution].label} ${state.fps}fps`
  });

  if (localInfo && localInfo.port) {
    const primaryIp = (localInfo.ips && localInfo.ips[0]) || '127.0.0.1';
    state.lanAddress = `${primaryIp}:${localInfo.port}`;
    connectLocalWebSocket(`ws://127.0.0.1:${localInfo.port}`);
  }

  // 2. Registra também o código curto no broker P2P PeerJS para amigos na internet entrarem apenas com os 4 dígitos
  initPeerJsHost(code);

  updateRoomUiState();
  showToast(`Sala #${code} criada! Passe o código para seus amigos.`);
}

async function joinRoomByInput(rawInput) {
  leaveRoom(true);

  const cleaned = rawInput.trim().replace(/^#/, '');
  state.isHost = false;
  state.inRoom = true;

  // Verifica se é IP:Porta (conexão direta LAN / Radmin VPN) ou Código curto (ex: 4829)
  if (cleaned.includes(':') || cleaned.includes('.')) {
    state.roomCode = cleaned;
    state.lanAddress = cleaned;
    connectLocalWebSocket(`ws://${cleaned}`);
    updateRoomUiState();
    showToast(`Conectando à sala ${cleaned}...`);
  } else {
    state.roomCode = cleaned.toUpperCase();
    // Verifica se essa sala também foi descoberta na rede local para conexão LAN instantânea
    let matchedLan = null;
    for (const r of state.lanRooms.values()) {
      if (String(r.roomCode).toUpperCase() === state.roomCode) {
        matchedLan = r;
        break;
      }
    }

    if (matchedLan) {
      state.lanAddress = `${matchedLan.hostIp}:${matchedLan.wsPort}`;
      connectLocalWebSocket(`ws://${state.lanAddress}`);
    } else {
      initPeerJsClient(state.roomCode);
    }

    updateRoomUiState();
    showToast(`Entrando na Sala #${state.roomCode}...`);
  }
}

function leaveRoom(silent = false) {
  if (state.isSharing && !silent) {
    stopScreenShare();
  }

  if (state.isHost) {
    window.desktopAPI.stopLocalRoom();
  }

  if (state.wsClient) {
    try { state.wsClient.close(); } catch (_) {}
    state.wsClient = null;
  }

  for (const pc of state.lanPeerConns.values()) {
    try { pc.close(); } catch (_) {}
  }
  state.lanPeerConns.clear();

  for (const call of state.peerJsMediaCalls.values()) {
    try { call.close(); } catch (_) {}
  }
  state.peerJsMediaCalls.clear();

  for (const conn of state.peerJsDataConns.values()) {
    try { conn.close(); } catch (_) {}
  }
  state.peerJsDataConns.clear();

  if (state.peerJs) {
    try { state.peerJs.destroy(); } catch (_) {}
    state.peerJs = null;
  }

  state.members.clear();
  state.inRoom = false;
  state.isHost = false;
  state.roomCode = '';
  state.lanAddress = '';

  if (state.watchingPeerId && state.watchingPeerId !== 'self') {
    clearStage();
  }

  el.remoteAudiosContainer.innerHTML = '';
  updateRoomUiState();
  renderMembersList();

  if (!silent) {
    showToast('Você saiu da sala.');
  }
}

function updateRoomUiState() {
  if (state.inRoom) {
    el.roomSetupPanel.classList.add('hidden');
    el.activeRoomPanel.classList.remove('hidden');
    el.btnLeaveRoom.classList.remove('hidden');

    el.activeRoomTitle.textContent = `Código: ${state.roomCode}`;
    el.activeRoomSub.textContent = state.isHost ? 'Hospedando Sala (P2P + LAN)' : 'Conectado na Sala';
    el.activeRoomLanHint.textContent = state.lanAddress
      ? `IP Direto LAN/VPN: ${state.lanAddress}`
      : 'Conexão Direta P2P pelo App';

    el.connMainText.textContent = `Sala Ativa #${state.roomCode}`;
    el.connSubText.textContent = 'Baixa latência • Sem navegador';
    el.netModeBadge.textContent = `Sala #${state.roomCode}`;
  } else {
    el.roomSetupPanel.classList.remove('hidden');
    el.activeRoomPanel.classList.add('hidden');
    el.btnLeaveRoom.classList.add('hidden');

    el.connMainText.textContent = 'Modo Local / Pronto';
    el.connSubText.textContent = 'Crie ou entre em uma sala para transmitir';
    el.netModeBadge.textContent = 'Pronto';
  }
}

// ============================================================================
// SINALIZAÇÃO VIA PEERJS (PARA CÓDIGO DE 4 DÍGITOS PELA INTERNET)
// ============================================================================

function getPeerJsBrokerId(code) {
  return `disc2-stream-room-${String(code).toLowerCase()}`;
}

function initPeerJsHost(code) {
  if (typeof Peer === 'undefined') return;
  try {
    const hostPeerId = getPeerJsBrokerId(code);
    const peer = new Peer(hostPeerId, {
      config: { iceServers: ICE_SERVERS }
    });
    state.peerJs = peer;

    peer.on('connection', (conn) => {
      setupPeerJsDataConnection(conn);
    });

    peer.on('call', (call) => {
      handleIncomingPeerJsCall(call);
    });

    peer.on('error', (err) => {
      console.warn('Aviso PeerJS Host:', err.type || err.message);
    });
  } catch (e) {
    console.warn('PeerJS indisponível (modo LAN ativo):', e.message);
  }
}

function initPeerJsClient(code) {
  if (typeof Peer === 'undefined') return;
  try {
    const peer = new Peer(undefined, {
      config: { iceServers: ICE_SERVERS }
    });
    state.peerJs = peer;

    peer.on('open', () => {
      const hostPeerId = getPeerJsBrokerId(code);
      const conn = peer.connect(hostPeerId, { reliable: true });
      setupPeerJsDataConnection(conn);
    });

    peer.on('call', (call) => {
      handleIncomingPeerJsCall(call);
    });

    peer.on('error', (err) => {
      showToast(`Aviso de conexão: verifique o código (${err.type || 'erro'})`);
    });
  } catch (e) {
    console.warn('Erro ao iniciar cliente PeerJS:', e.message);
  }
}

function setupPeerJsDataConnection(conn) {
  conn.on('open', () => {
    state.peerJsDataConns.set(conn.peer, conn);
    conn.send({
      type: 'hello',
      peerId: state.myId,
      peerJsId: state.peerJs ? state.peerJs.id : '',
      userName: state.userName,
      isSharing: state.isSharing,
      streamLabel: `${RESOLUTION_MAP[state.resolution].label} ${state.fps}fps`
    });

    // Se já estamos transmitindo, inicia envio do stream para este participante
    if (state.isSharing && state.localScreenStream && state.peerJs) {
      callPeerWithScreen(conn.peer);
    }
  });

  conn.on('data', (data) => {
    if (!data || !data.type) return;

    if (data.type === 'hello' || data.type === 'state-update') {
      const remoteKey = conn.peer;
      const existing = state.members.get(remoteKey) || {};
      state.members.set(remoteKey, {
        ...existing,
        peerId: remoteKey,
        userName: data.userName || 'Amigo',
        isSharing: Boolean(data.isSharing),
        streamLabel: data.streamLabel || '1080p 60fps'
      });

      if (!data.isSharing && state.watchingPeerId === remoteKey) {
        clearStage();
      }
      renderMembersList();
    }
  });

  conn.on('close', () => {
    state.peerJsDataConns.delete(conn.peer);
    state.members.delete(conn.peer);
    if (state.watchingPeerId === conn.peer) {
      clearStage();
    }
    renderMembersList();
  });
}

function callPeerWithScreen(remotePeerJsId) {
  if (!state.peerJs || !state.localScreenStream) return;
  try {
    const call = state.peerJs.call(remotePeerJsId, state.localScreenStream, {
      metadata: {
        userName: state.userName,
        sourceName: state.selectedSourceName || 'Tela',
        kind: 'screen'
      }
    });
    state.peerJsMediaCalls.set(remotePeerJsId, call);
    setTimeout(() => {
      if (call.peerConnection) {
        configureSenderOnPeerConnection(call.peerConnection);
      }
    }, 600);
  } catch (e) {
    console.warn('Erro ao enviar stream via PeerJS:', e.message);
  }
}

function handleIncomingPeerJsCall(call) {
  call.answer();
  call.on('stream', (remoteStream) => {
    const remoteKey = call.peer;
    const member = state.members.get(remoteKey) || {
      peerId: remoteKey,
      userName: (call.metadata && call.metadata.userName) || 'Participante'
    };
    member.isSharing = true;
    member.stream = remoteStream;
    state.members.set(remoteKey, member);

    // Abre automaticamente a transmissão recebida caso não esteja vendo outra
    if (!state.watchingPeerId || state.watchingPeerId !== 'self') {
      watchStream(remoteKey, remoteStream, `${member.userName} — Transmissão Ao Vivo`);
    }
    renderMembersList();
  });

  call.on('close', () => {
    if (state.watchingPeerId === call.peer) {
      clearStage();
    }
  });
}

// ============================================================================
// SINALIZAÇÃO LOCAL WEBSOCKET + RTCPeerConnection (MODO LAN / RADMIN / ZEROTIER)
// ============================================================================

function connectLocalWebSocket(wsUrl) {
  try {
    const ws = new WebSocket(wsUrl);
    state.wsClient = ws;

    ws.onopen = () => {
      ws.send(JSON.stringify({
        type: 'register',
        peerId: state.myId,
        userName: state.userName
      }));
    };

    ws.onmessage = async (event) => {
      const msg = JSON.parse(event.data);

      if (msg.type === 'room-peers') {
        for (const p of msg.peers) {
          state.members.set(p.peerId, {
            peerId: p.peerId,
            userName: p.userName,
            isSharing: false
          });
          await createLanPeerConnection(p.peerId, true);
        }
        renderMembersList();
      } else if (msg.type === 'peer-joined') {
        state.members.set(msg.peerId, {
          peerId: msg.peerId,
          userName: msg.userName,
          isSharing: false
        });
        renderMembersList();
        broadcastMyState();
      } else if (msg.type === 'peer-left') {
        const pc = state.lanPeerConns.get(msg.peerId);
        if (pc) pc.close();
        state.lanPeerConns.delete(msg.peerId);
        state.members.delete(msg.peerId);
        if (state.watchingPeerId === msg.peerId) {
          clearStage();
        }
        renderMembersList();
      } else if (msg.type === 'offer') {
        const pc = await createLanPeerConnection(msg.fromPeerId, false);
        await pc.setRemoteDescription(new RTCSessionDescription(msg.sdp));
        const answer = await pc.createAnswer();
        await pc.setLocalDescription(answer);
        ws.send(JSON.stringify({
          type: 'answer',
          targetPeerId: msg.fromPeerId,
          sdp: pc.localDescription
        }));
      } else if (msg.type === 'answer') {
        const pc = state.lanPeerConns.get(msg.fromPeerId);
        if (pc) {
          await pc.setRemoteDescription(new RTCSessionDescription(msg.sdp));
        }
      } else if (msg.type === 'ice-candidate') {
        const pc = state.lanPeerConns.get(msg.fromPeerId);
        if (pc && msg.candidate) {
          await pc.addIceCandidate(new RTCIceCandidate(msg.candidate)).catch(() => {});
        }
      } else if (msg.type === 'broadcast-state') {
        const m = state.members.get(msg.fromPeerId) || { peerId: msg.fromPeerId };
        m.userName = msg.userName || m.userName || 'Participante';
        m.isSharing = Boolean(msg.isSharing);
        m.streamLabel = msg.streamLabel || '1080p 60fps';
        state.members.set(msg.fromPeerId, m);
        if (!m.isSharing && state.watchingPeerId === msg.fromPeerId) {
          clearStage();
        }
        renderMembersList();
      }
    };
  } catch (e) {
    console.warn('Falha ao conectar WS local:', e.message);
  }
}

async function createLanPeerConnection(remotePeerId, isInitiator) {
  let pc = state.lanPeerConns.get(remotePeerId);
  if (pc) {
    try { pc.close(); } catch (_) {}
  }

  pc = new RTCPeerConnection({ iceServers: ICE_SERVERS });
  state.lanPeerConns.set(remotePeerId, pc);

  // Adiciona trilhas de tela se estiver transmitindo
  if (state.isSharing && state.localScreenStream) {
    state.localScreenStream.getTracks().forEach((track) => {
      pc.addTrack(track, state.localScreenStream);
    });
    setTimeout(() => configureSenderOnPeerConnection(pc), 250);
  }

  // Adiciona trilha de microfone se estiver ativo
  if (state.micEnabled && state.localMicStream) {
    state.localMicStream.getAudioTracks().forEach((track) => {
      pc.addTrack(track, state.localMicStream);
    });
  }

  pc.onicecandidate = (e) => {
    if (e.candidate && state.wsClient && state.wsClient.readyState === 1) {
      state.wsClient.send(JSON.stringify({
        type: 'ice-candidate',
        targetPeerId: remotePeerId,
        candidate: e.candidate
      }));
    }
  };

  pc.ontrack = (e) => {
    const stream = e.streams && e.streams[0];
    if (!stream) return;

    if (e.track.kind === 'video') {
      const m = state.members.get(remotePeerId) || { peerId: remotePeerId, userName: 'Participante' };
      m.isSharing = true;
      m.stream = stream;
      state.members.set(remotePeerId, m);

      if (!state.watchingPeerId || state.watchingPeerId !== 'self') {
        watchStream(remotePeerId, stream, `${m.userName} — Transmissão Ao Vivo`);
      }
      renderMembersList();
    } else if (e.track.kind === 'audio') {
      attachRemoteAudio(remotePeerId, stream);
    }
  };

  if (isInitiator) {
    const offer = await pc.createOffer();
    await pc.setLocalDescription(offer);
    if (state.wsClient && state.wsClient.readyState === 1) {
      state.wsClient.send(JSON.stringify({
        type: 'offer',
        targetPeerId: remotePeerId,
        sdp: pc.localDescription
      }));
    }
  }

  return pc;
}

function publishStreamToAllPeers() {
  // 1. Envia para conexões PeerJS
  for (const remotePeerJsId of state.peerJsDataConns.keys()) {
    callPeerWithScreen(remotePeerJsId);
  }

  // 2. Renegocia conexões LAN WebSocket
  for (const remotePeerId of state.lanPeerConns.keys()) {
    createLanPeerConnection(remotePeerId, true);
  }
}

function broadcastMyState() {
  const res = RESOLUTION_MAP[state.resolution] || RESOLUTION_MAP['1080'];
  const payload = {
    type: 'state-update',
    userName: state.userName,
    isSharing: state.isSharing,
    streamLabel: `${res.label} ${state.fps}fps`
  };

  for (const conn of state.peerJsDataConns.values()) {
    try { conn.send(payload); } catch (_) {}
  }

  if (state.wsClient && state.wsClient.readyState === 1) {
    state.wsClient.send(JSON.stringify({
      type: 'broadcast-state',
      userName: state.userName,
      isSharing: state.isSharing,
      streamLabel: `${res.label} ${state.fps}fps`
    }));
  }
}

// ============================================================================
// OTIMIZAÇÃO DE ENCODING WEBRTC (BITRATE + 60FPS HARDWARE ACCELERATED)
// ============================================================================

async function configureSenderOnPeerConnection(pc) {
  if (!pc) return;
  const senders = pc.getSenders ? pc.getSenders() : [];
  for (const sender of senders) {
    if (sender.track && sender.track.kind === 'video') {
      try {
        const params = sender.getParameters();
        if (!params.encodings || params.encodings.length === 0) {
          params.encodings = [{}];
        }
        params.encodings[0].maxBitrate = state.bitrateKbps * 1000;
        params.encodings[0].maxFramerate = state.fps;
        params.encodings[0].networkPriority = 'high';
        params.degradationPreference =
          state.contentHint === 'detail' ? 'maintain-resolution' : 'maintain-framerate';
        await sender.setParameters(params);
      } catch (_) {}
    }
  }
}

async function applyAllSendersEncodingParams() {
  for (const call of state.peerJsMediaCalls.values()) {
    if (call.peerConnection) {
      await configureSenderOnPeerConnection(call.peerConnection);
    }
  }
  for (const pc of state.lanPeerConns.values()) {
    await configureSenderOnPeerConnection(pc);
  }
}

// ============================================================================
// MICROFONE E ÁUDIO DA SALA
// ============================================================================

async function toggleMicrophone() {
  if (!state.micEnabled) {
    try {
      state.localMicStream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true
        },
        video: false
      });
      state.micEnabled = true;
      el.btnToggleMicSmall.classList.remove('muted');
      el.btnDockMicLabel.textContent = 'Mic: Ligado';
      showToast('Microfone ativado.');

      for (const remotePeerId of state.lanPeerConns.keys()) {
        createLanPeerConnection(remotePeerId, true);
      }
    } catch (_) {
      showToast('Nenhum microfone disponível.');
    }
  } else {
    if (state.localMicStream) {
      state.localMicStream.getTracks().forEach((t) => t.stop());
      state.localMicStream = null;
    }
    state.micEnabled = false;
    el.btnToggleMicSmall.classList.add('muted');
    el.btnDockMicLabel.textContent = 'Mic: Desligado';
    showToast('Microfone silenciado.');
  }
}

function toggleDeafen() {
  state.deafened = !state.deafened;
  el.btnToggleDeafenSmall.classList.toggle('muted', state.deafened);

  if (state.watchingPeerId && state.watchingPeerId !== 'self') {
    el.streamVideo.muted = state.deafened;
  }
  el.remoteAudiosContainer.querySelectorAll('audio').forEach((a) => {
    a.muted = state.deafened;
  });
  showToast(state.deafened ? 'Áudio da sala silenciado.' : 'Áudio da sala restaurado.');
}

function attachRemoteAudio(peerId, stream) {
  let audioEl = document.getElementById(`audio-${peerId}`);
  if (!audioEl) {
    audioEl = document.createElement('audio');
    audioEl.id = `audio-${peerId}`;
    audioEl.autoplay = true;
    el.remoteAudiosContainer.appendChild(audioEl);
  }
  audioEl.srcObject = stream;
  audioEl.muted = state.deafened;
}

// ============================================================================
// RENDERIZAÇÃO DE LISTAS (MEMBROS + SALAS LAN) E TELEMETRIA
// ============================================================================

function renderMembersList() {
  el.membersList.innerHTML = '';
  const totalCount = 1 + state.members.size;
  el.memberCountBadge.textContent = String(totalCount);

  // Linha do próprio usuário
  const selfRes = RESOLUTION_MAP[state.resolution] || RESOLUTION_MAP['1080'];
  const selfRow = document.createElement('div');
  selfRow.className = 'member-row' + (state.watchingPeerId === 'self' ? ' watching' : '');
  selfRow.innerHTML = `
    <div class="member-left">
      <div class="member-avatar" style="background: var(--green)">${escapeHtml((state.userName[0] || 'V').toUpperCase())}</div>
      <div>
        <div class="member-name">${escapeHtml(state.userName)} (Você)</div>
        <div class="member-sub">${state.isSharing ? `Transmitindo ${selfRes.label} ${state.fps}fps` : 'Na sala'}</div>
      </div>
    </div>
    ${state.isSharing ? '<span class="live-pill">AO VIVO</span>' : ''}
  `;
  selfRow.addEventListener('click', () => {
    if (state.isSharing && state.localScreenStream) {
      watchStream('self', state.localScreenStream, `${state.userName} — ${state.selectedSourceName || 'Tela'}`);
    }
  });
  el.membersList.appendChild(selfRow);

  // Participantes remotos
  for (const [peerId, m] of state.members.entries()) {
    const row = document.createElement('div');
    row.className = 'member-row' + (state.watchingPeerId === peerId ? ' watching' : '');
    const initial = ((m.userName || 'P')[0] || 'P').toUpperCase();
    row.innerHTML = `
      <div class="member-left">
        <div class="member-avatar">${escapeHtml(initial)}</div>
        <div>
          <div class="member-name">${escapeHtml(m.userName || 'Participante')}</div>
          <div class="member-sub">${m.isSharing ? `Transmitindo ${escapeHtml(m.streamLabel || 'Tela')} — Clique p/ assistir` : 'Conectado'}</div>
        </div>
      </div>
      ${m.isSharing ? '<span class="live-pill">AO VIVO</span>' : ''}
    `;
    row.addEventListener('click', () => {
      if (m.stream) {
        watchStream(peerId, m.stream, `${m.userName} — Transmissão Ao Vivo`);
      } else if (m.isSharing) {
        showToast('Sincronizando vídeo da transmissão...');
      }
    });
    el.membersList.appendChild(row);
  }
}

function renderLanRooms() {
  const now = Date.now();
  for (const [k, r] of state.lanRooms.entries()) {
    if (now - r.timestamp > 7000) {
      state.lanRooms.delete(k);
    }
  }

  el.lanCountBadge.textContent = String(state.lanRooms.size);
  el.lanRoomsList.innerHTML = '';

  if (state.lanRooms.size === 0) {
    el.lanRoomsList.innerHTML = '<div class="empty-hint">Nenhuma outra sala aberta na rede local.</div>';
    return;
  }

  for (const r of state.lanRooms.values()) {
    const item = document.createElement('div');
    item.className = 'lan-room-item';
    item.innerHTML = `
      <div class="lan-room-meta">
        <div class="lan-room-title">Sala #${escapeHtml(r.roomCode)} (${escapeHtml(r.hostName)})</div>
        <div class="lan-room-sub">${r.hasStream ? `🔴 Ao Vivo (${escapeHtml(r.streamLabel)})` : `${r.memberCount} na sala`}</div>
      </div>
      <button class="btn btn-primary btn-sm">Entrar</button>
    `;
    item.querySelector('button').addEventListener('click', () => {
      joinRoomByInput(`${r.hostIp}:${r.wsPort}`);
    });
    el.lanRoomsList.appendChild(item);
  }
}

async function periodicTelemetryTick() {
  renderLanRooms();

  try {
    const sys = await window.desktopAPI.getSystemInfo();
    if (sys && sys.ramMB) {
      el.sysRamBadge.textContent = `RAM: ${sys.ramMB} MB`;
    }
  } catch (_) {}

  if (!state.watchingPeerId) return;

  const vw = el.streamVideo.videoWidth;
  const vh = el.streamVideo.videoHeight;
  const resCfg = RESOLUTION_MAP[state.resolution] || RESOLUTION_MAP['1080'];
  el.hudResolution.textContent = vw && vh ? `${vw}×${vh}` : `${resCfg.width}×${resCfg.height}`;

  let measuredFps = state.fps;
  let measuredMbps = (state.bitrateKbps / 1000).toFixed(1);
  let measuredPing = '< 5';

  const activePc = getFirstActivePeerConnection();
  if (activePc) {
    try {
      const stats = await activePc.getStats();
      stats.forEach((report) => {
        if ((report.type === 'outbound-rtp' || report.type === 'inbound-rtp') && report.kind === 'video') {
          if (report.framesPerSecond) {
            measuredFps = Math.round(report.framesPerSecond);
          }
          const bytes = report.bytesSent || report.bytesReceived || 0;
          if (state.lastStatsTimestamp > 0 && bytes >= state.lastBytesTransferred) {
            const dt = (report.timestamp - state.lastStatsTimestamp) / 1000;
            if (dt > 0) {
              const bps = ((bytes - state.lastBytesTransferred) * 8) / dt;
              measuredMbps = (bps / (1000 * 1000)).toFixed(1);
            }
          }
          state.lastBytesTransferred = bytes;
          state.lastStatsTimestamp = report.timestamp;
        }
        if (report.type === 'candidate-pair' && report.state === 'succeeded' && report.currentRoundTripTime != null) {
          measuredPing = Math.round(report.currentRoundTripTime * 1000);
        }
      });
    } catch (_) {}
  } else if (state.watchingPeerId === 'self' && state.localScreenStream) {
    const vt = state.localScreenStream.getVideoTracks()[0];
    if (vt && vt.getSettings) {
      const s = vt.getSettings();
      if (s.frameRate) measuredFps = Math.round(s.frameRate);
    }
  }

  el.hudFps.textContent = `${measuredFps} FPS`;
  el.hudBitrate.textContent = `${measuredMbps} Mbps`;
  el.hudPing.textContent = `${measuredPing} ms`;
}

function getFirstActivePeerConnection() {
  for (const call of state.peerJsMediaCalls.values()) {
    if (call.peerConnection) return call.peerConnection;
  }
  for (const pc of state.lanPeerConns.values()) {
    return pc;
  }
  return null;
}

function escapeHtml(str) {
  return String(str || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

window.addEventListener('DOMContentLoaded', init);
