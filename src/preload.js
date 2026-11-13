const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('desktopAPI', {
  minimize: () => ipcRenderer.send('win-minimize'),
  maximize: () => ipcRenderer.send('win-maximize'),
  close: () => ipcRenderer.send('win-close'),

  getCaptureSources: () => ipcRenderer.invoke('get-capture-sources'),
  setCaptureTarget: (config) => ipcRenderer.invoke('set-capture-target', config),

  startLocalRoom: (roomMeta) => ipcRenderer.invoke('start-local-room', roomMeta),
  updateLocalRoom: (updates) => ipcRenderer.invoke('update-local-room', updates),
  stopLocalRoom: () => ipcRenderer.invoke('stop-local-room'),

  getSystemInfo: () => ipcRenderer.invoke('get-system-info'),

  onLanRoomDiscovered: (callback) => {
    const handler = (_, data) => callback(data);
    ipcRenderer.on('lan-room-discovered', handler);
    return () => ipcRenderer.removeListener('lan-room-discovered', handler);
  }
});
