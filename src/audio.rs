use std::ffi::OsStr;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use winapi::shared::guiddef::{GUID, REFIID};
use winapi::shared::minwindef::{BYTE, DWORD, LPVOID, UINT, ULONG, WORD};
use winapi::shared::mmreg::{WAVEFORMATEX, WAVE_FORMAT_PCM};
use winapi::shared::winerror::{E_NOINTERFACE, S_OK};
use winapi::um::winnt::HRESULT;
use winapi::um::audioclient::{IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_SILENT};
use winapi::um::audiosessiontypes::{AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK};
use winapi::um::combaseapi::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL};
use winapi::um::libloaderapi::{GetProcAddress, LoadLibraryA};
use winapi::um::mmdeviceapi::{eConsole, eRender, IMMDevice, IMMDeviceEnumerator};
use winapi::um::mmeapi::{
    waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader,
    waveOutWrite,
};
use winapi::um::mmsystem::{CALLBACK_NULL, HWAVEOUT, MMSYSERR_NOERROR, WAVEHDR, WAVE_MAPPER};
use winapi::um::objbase::COINIT_MULTITHREADED;
use winapi::um::synchapi::{CreateEventW, SetEvent, WaitForSingleObject};
use winapi::um::unknwnbase::{IUnknown, IUnknownVtbl};

pub const AUDIO_SAMPLE_RATE: u32 = 24000;
pub const AUDIO_CHANNELS: u16 = 2;
pub const AUDIO_BITS_PER_SAMPLE: u16 = 16;

const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: DWORD = 0x8000_0000;
const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: DWORD = 0x0800_0000;

const CLSID_MMDEVICE_ENUMERATOR: GUID = GUID {
    Data1: 0xBCDE0395,
    Data2: 0xE52F,
    Data3: 0x467C,
    Data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
};

const IID_IMMDEVICE_ENUMERATOR: GUID = GUID {
    Data1: 0xA95664D2,
    Data2: 0x9614,
    Data3: 0x4F35,
    Data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
};

const IID_IAUDIO_CLIENT: GUID = GUID {
    Data1: 0x1CB9AD4C,
    Data2: 0xDBFA,
    Data3: 0x4C32,
    Data4: [0xB1, 0x78, 0xC2, 0xF5, 0x68, 0xA7, 0x03, 0xB2],
};

const IID_IAUDIO_CAPTURE_CLIENT: GUID = GUID {
    Data1: 0xC8ADBD64,
    Data2: 0xE71E,
    Data3: 0x48A0,
    Data4: [0xA4, 0xDE, 0x18, 0x5C, 0x39, 0x5C, 0xD3, 0x17],
};

const IID_IUNKNOWN: GUID = GUID {
    Data1: 0x00000000,
    Data2: 0x0000,
    Data3: 0x0000,
    Data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

// Interfaces para captura isolada por processo (Windows 10 2004+ / Windows 11 Process Loopback)
const IID_IACTIVATE_AUDIO_INTERFACE_COMPLETION_HANDLER: GUID = GUID {
    Data1: 0x41D949AB,
    Data2: 0x9862,
    Data3: 0x444A,
    Data4: [0x80, 0xF6, 0xC2, 0x61, 0x33, 0x4D, 0xA5, 0xEB],
};

const IID_IAGILE_OBJECT: GUID = GUID {
    Data1: 0x94EA2B94,
    Data2: 0xE9CC,
    Data3: 0x49E0,
    Data4: [0xC0, 0xFF, 0xEE, 0x64, 0xCA, 0x8F, 0x5B, 0x90],
};

#[repr(C)]
struct AudioClientProcessLoopbackParams {
    target_process_id: DWORD,
    process_loopback_mode: u32, // 0 = PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
}

#[repr(C)]
struct AudioClientActivationParams {
    activation_type: u32, // 1 = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK
    process_loopback_params: AudioClientProcessLoopbackParams,
}

#[repr(C)]
struct BlobData {
    cb_size: ULONG,
    p_blob_data: *mut BYTE,
}

#[repr(C)]
struct PropVariantBlob {
    vt: u16, // VT_BLOB = 65
    w_reserved1: u16,
    w_reserved2: u16,
    w_reserved3: u16,
    blob: BlobData,
}

#[repr(C)]
struct IActivateAudioInterfaceAsyncOperationVtbl {
    parent: IUnknownVtbl,
    get_activate_result: unsafe extern "system" fn(
        this: *mut IActivateAudioInterfaceAsyncOperation,
        activate_result: *mut HRESULT,
        activated_interface: *mut *mut IUnknown,
    ) -> HRESULT,
}

#[repr(C)]
struct IActivateAudioInterfaceAsyncOperation {
    lp_vtbl: *const IActivateAudioInterfaceAsyncOperationVtbl,
}

#[repr(C)]
struct IActivateAudioInterfaceCompletionHandlerVtbl {
    parent: IUnknownVtbl,
    activate_completed: unsafe extern "system" fn(
        this: *mut CompletionHandlerCom,
        activate_operation: *mut IActivateAudioInterfaceAsyncOperation,
    ) -> HRESULT,
}

#[repr(C)]
struct CompletionHandlerCom {
    lp_vtbl: *const IActivateAudioInterfaceCompletionHandlerVtbl,
    ref_count: AtomicU32,
    h_event: LPVOID,
}

unsafe extern "system" fn handler_query_interface(
    this: *mut IUnknown,
    riid: REFIID,
    ppv: *mut LPVOID,
) -> HRESULT {
    if ppv.is_null() {
        return E_NOINTERFACE;
    }
    let r = &*riid;
    if guid_eq(r, &IID_IUNKNOWN)
        || guid_eq(r, &IID_IACTIVATE_AUDIO_INTERFACE_COMPLETION_HANDLER)
        || guid_eq(r, &IID_IAGILE_OBJECT)
    {
        *ppv = this as LPVOID;
        handler_add_ref(this);
        S_OK
    } else {
        *ppv = null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn handler_add_ref(this: *mut IUnknown) -> ULONG {
    let me = &*(this as *mut CompletionHandlerCom);
    me.ref_count.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn handler_release(this: *mut IUnknown) -> ULONG {
    let me = &*(this as *mut CompletionHandlerCom);
    me.ref_count.fetch_sub(1, Ordering::Relaxed) - 1
}

unsafe extern "system" fn handler_activate_completed(
    this: *mut CompletionHandlerCom,
    _op: *mut IActivateAudioInterfaceAsyncOperation,
) -> HRESULT {
    let me = &*this;
    if !me.h_event.is_null() {
        SetEvent(me.h_event);
    }
    S_OK
}

static COMPLETION_HANDLER_VTBL: IActivateAudioInterfaceCompletionHandlerVtbl =
    IActivateAudioInterfaceCompletionHandlerVtbl {
        parent: IUnknownVtbl {
            QueryInterface: handler_query_interface,
            AddRef: handler_add_ref,
            Release: handler_release,
        },
        activate_completed: handler_activate_completed,
    };

fn guid_eq(a: &GUID, b: &GUID) -> bool {
    a.Data1 == b.Data1 && a.Data2 == b.Data2 && a.Data3 == b.Data3 && a.Data4 == b.Data4
}

type ActivateAudioInterfaceAsyncFn = unsafe extern "system" fn(
    device_interface_path: *const u16,
    riid: REFIID,
    activation_params: *const PropVariantBlob,
    completion_handler: *mut CompletionHandlerCom,
    activation_operation: *mut *mut IActivateAudioInterfaceAsyncOperation,
) -> HRESULT;

/// Tenta ativar o cliente de áudio isolado APENAS para o processo `target_pid` (ex: apenas o áudio do Valorant)
unsafe fn activate_process_specific_audio_client(target_pid: u32) -> *mut IAudioClient {
    let h_mod = LoadLibraryA(b"Mmdevapi.dll\0".as_ptr() as *const i8);
    if h_mod.is_null() {
        return null_mut();
    }

    let proc = GetProcAddress(
        h_mod,
        b"ActivateAudioInterfaceAsync\0".as_ptr() as *const i8,
    );
    if proc.is_null() {
        return null_mut();
    }

    let activate_fn: ActivateAudioInterfaceAsyncFn = std::mem::transmute(proc);

    let h_event = CreateEventW(null_mut(), 1, 0, null_mut());
    if h_event.is_null() {
        return null_mut();
    }

    let mut handler = CompletionHandlerCom {
        lp_vtbl: &COMPLETION_HANDLER_VTBL,
        ref_count: AtomicU32::new(1),
        h_event,
    };

    let mut act_params = AudioClientActivationParams {
        activation_type: 1, // AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK
        process_loopback_params: AudioClientProcessLoopbackParams {
            target_process_id: target_pid,
            process_loopback_mode: 0, // PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
        },
    };

    let prop_var = PropVariantBlob {
        vt: 65, // VT_BLOB
        w_reserved1: 0,
        w_reserved2: 0,
        w_reserved3: 0,
        blob: BlobData {
            cb_size: size_of::<AudioClientActivationParams>() as ULONG,
            p_blob_data: &mut act_params as *mut _ as *mut BYTE,
        },
    };

    let dev_path: Vec<u16> = OsStr::new("VAD\\Process_Loopback\0")
        .encode_wide()
        .collect();

    let mut op: *mut IActivateAudioInterfaceAsyncOperation = null_mut();
    let hr = activate_fn(
        dev_path.as_ptr(),
        &IID_IAUDIO_CLIENT,
        &prop_var,
        &mut handler,
        &mut op,
    );

    if hr != S_OK || op.is_null() {
        winapi::um::handleapi::CloseHandle(h_event);
        return null_mut();
    }

    WaitForSingleObject(h_event, 2000);
    winapi::um::handleapi::CloseHandle(h_event);

    let mut activate_hr: HRESULT = -1;
    let mut unk: *mut IUnknown = null_mut();
    let hr2 = ((*(*op).lp_vtbl).get_activate_result)(op, &mut activate_hr, &mut unk);
    ((*(*op).lp_vtbl).parent.Release)(op as *mut IUnknown);

    if hr2 == S_OK && activate_hr == S_OK && !unk.is_null() {
        unk as *mut IAudioClient
    } else {
        null_mut()
    }
}

/// Abre o cliente de áudio global do sistema (usado apenas quando transmite Tela Inteira)
unsafe fn activate_system_loopback_audio_client() -> *mut IAudioClient {
    let mut enumerator: *mut IMMDeviceEnumerator = null_mut();
    let hr = CoCreateInstance(
        &CLSID_MMDEVICE_ENUMERATOR,
        null_mut(),
        CLSCTX_ALL,
        &IID_IMMDEVICE_ENUMERATOR,
        &mut enumerator as *mut _ as *mut _,
    );
    if hr != S_OK || enumerator.is_null() {
        return null_mut();
    }

    let mut device: *mut IMMDevice = null_mut();
    let hr = (*enumerator).GetDefaultAudioEndpoint(eRender, eConsole, &mut device);
    if hr != S_OK || device.is_null() {
        (*enumerator).Release();
        return null_mut();
    }

    let mut audio_client: *mut IAudioClient = null_mut();
    let _ = (*device).Activate(
        &IID_IAUDIO_CLIENT,
        CLSCTX_ALL,
        null_mut(),
        &mut audio_client as *mut _ as *mut _,
    );

    (*device).Release();
    (*enumerator).Release();
    audio_client
}

/// Captura o áudio isolado do jogo/aplicativo selecionado (`target_pid > 0`) ou da tela inteira (`target_pid == 0`)
pub fn spawn_system_audio_loopback<F>(
    stop_flag: Arc<AtomicBool>,
    audio_enabled: Arc<AtomicBool>,
    target_pid: Arc<AtomicU32>,
    mut on_pcm_chunk: F,
) where
    F: FnMut(&[u8]) + Send + 'static,
{
    thread::spawn(move || unsafe {
        let _ = CoInitializeEx(null_mut(), COINIT_MULTITHREADED);

        while !stop_flag.load(Ordering::Relaxed) {
            let current_pid = target_pid.load(Ordering::Relaxed);

            // Se current_pid > 0 (ex: Valorant, Chrome, etc.), captura SOMENTE o áudio daquele processo!
            let mut audio_client = if current_pid > 0 {
                activate_process_specific_audio_client(current_pid)
            } else {
                null_mut()
            };

            if audio_client.is_null() {
                audio_client = activate_system_loopback_audio_client();
            }

            if audio_client.is_null() {
                thread::sleep(Duration::from_millis(300));
                continue;
            }

            let block_align = (AUDIO_CHANNELS * (AUDIO_BITS_PER_SAMPLE / 8)) as WORD;
            let wfx = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM,
                nChannels: AUDIO_CHANNELS,
                nSamplesPerSec: AUDIO_SAMPLE_RATE,
                nAvgBytesPerSec: AUDIO_SAMPLE_RATE * block_align as u32,
                nBlockAlign: block_align,
                wBitsPerSample: AUDIO_BITS_PER_SAMPLE,
                cbSize: 0,
            };

            let hns_requested_duration: i64 = 500_000;
            let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;

            if (*audio_client).Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                flags,
                hns_requested_duration,
                0,
                &wfx,
                null_mut(),
            ) != S_OK
            {
                (*audio_client).Release();
                thread::sleep(Duration::from_millis(300));
                continue;
            }

            let mut capture_client: *mut IAudioCaptureClient = null_mut();
            if (*audio_client).GetService(
                &IID_IAUDIO_CAPTURE_CLIENT,
                &mut capture_client as *mut _ as *mut _,
            ) != S_OK
                || capture_client.is_null()
            {
                (*audio_client).Release();
                thread::sleep(Duration::from_millis(300));
                continue;
            }

            if (*audio_client).Start() != S_OK {
                (*capture_client).Release();
                (*audio_client).Release();
                thread::sleep(Duration::from_millis(300));
                continue;
            }

            let mut accum_buf: Vec<u8> = Vec::with_capacity(4800);

            // Permanece capturando enquanto a live estiver ativa e o usuário não trocar de aplicativo
            while !stop_flag.load(Ordering::Relaxed)
                && target_pid.load(Ordering::Relaxed) == current_pid
            {
                thread::sleep(Duration::from_millis(12));

                let mut packet_length: UINT = 0;
                if (*capture_client).GetNextPacketSize(&mut packet_length) != S_OK {
                    break;
                }

                while packet_length != 0 {
                    let mut data_ptr: *mut BYTE = null_mut();
                    let mut num_frames: UINT = 0;
                    let mut buf_flags: DWORD = 0;

                    if (*capture_client).GetBuffer(
                        &mut data_ptr,
                        &mut num_frames,
                        &mut buf_flags,
                        null_mut(),
                        null_mut(),
                    ) != S_OK
                    {
                        break;
                    }

                    let byte_count = (num_frames as usize) * (block_align as usize);
                    if audio_enabled.load(Ordering::Relaxed) && byte_count > 0 {
                        if (buf_flags & AUDCLNT_BUFFERFLAGS_SILENT) == 0 && !data_ptr.is_null() {
                            let slice = std::slice::from_raw_parts(data_ptr, byte_count);
                            accum_buf.extend_from_slice(slice);
                        } else {
                            accum_buf.resize(accum_buf.len() + byte_count, 0);
                        }

                        if accum_buf.len() >= 1920 {
                            let has_signal = accum_buf.iter().any(|&b| b != 0);
                            if has_signal {
                                on_pcm_chunk(&accum_buf);
                            }
                            accum_buf.clear();
                        }
                    } else {
                        accum_buf.clear();
                    }

                    let _ = (*capture_client).ReleaseBuffer(num_frames);
                    if (*capture_client).GetNextPacketSize(&mut packet_length) != S_OK {
                        break;
                    }
                }
            }

            let _ = (*audio_client).Stop();
            (*capture_client).Release();
            (*audio_client).Release();
        }
    });
}

pub struct StreamAudioPlayer {
    pub tx: SyncSender<Vec<u8>>,
    stop_flag: Arc<AtomicBool>,
}

impl StreamAudioPlayer {
    pub fn start(volume_percent: Arc<AtomicU32>, muted: Arc<AtomicBool>) -> Self {
        let (tx, rx) = sync_channel::<Vec<u8>>(16);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);

        thread::spawn(move || unsafe {
            let block_align = (AUDIO_CHANNELS * (AUDIO_BITS_PER_SAMPLE / 8)) as WORD;
            let wfx = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM,
                nChannels: AUDIO_CHANNELS,
                nSamplesPerSec: AUDIO_SAMPLE_RATE,
                nAvgBytesPerSec: AUDIO_SAMPLE_RATE * block_align as u32,
                nBlockAlign: block_align,
                wBitsPerSample: AUDIO_BITS_PER_SAMPLE,
                cbSize: 0,
            };

            let mut h_wave_out: HWAVEOUT = null_mut();
            if waveOutOpen(
                &mut h_wave_out,
                WAVE_MAPPER,
                &wfx,
                0,
                0,
                CALLBACK_NULL,
            ) != MMSYSERR_NOERROR
                || h_wave_out.is_null()
            {
                return;
            }

            const RING_SIZE: usize = 12;
            let mut headers: Vec<WAVEHDR> = (0..RING_SIZE).map(|_| zeroed()).collect();
            let mut buffers: Vec<Vec<u8>> = (0..RING_SIZE).map(|_| Vec::new()).collect();
            let mut ring_idx = 0;

            while !stop_clone.load(Ordering::Relaxed) {
                let Ok(mut pcm) = rx.recv_timeout(Duration::from_millis(200)) else {
                    continue;
                };

                if muted.load(Ordering::Relaxed) {
                    continue;
                }

                let vol = volume_percent.load(Ordering::Relaxed) as f32 / 100.0;
                if (vol - 1.0).abs() > 0.01 {
                    for sample_bytes in pcm.chunks_exact_mut(2) {
                        let s = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
                        let scaled = ((s as f32) * vol).clamp(-32768.0, 32767.0) as i16;
                        let out = scaled.to_le_bytes();
                        sample_bytes[0] = out[0];
                        sample_bytes[1] = out[1];
                    }
                }

                let hdr = &mut headers[ring_idx];
                if (hdr.dwFlags & 1) != 0 {
                    let _ = waveOutUnprepareHeader(
                        h_wave_out,
                        hdr as *mut WAVEHDR,
                        size_of::<WAVEHDR>() as UINT,
                    );
                }

                buffers[ring_idx] = pcm;
                *hdr = zeroed();
                hdr.lpData = buffers[ring_idx].as_mut_ptr() as *mut i8;
                hdr.dwBufferLength = buffers[ring_idx].len() as DWORD;

                if waveOutPrepareHeader(
                    h_wave_out,
                    hdr as *mut WAVEHDR,
                    size_of::<WAVEHDR>() as UINT,
                ) == MMSYSERR_NOERROR
                {
                    let _ = waveOutWrite(
                        h_wave_out,
                        hdr as *mut WAVEHDR,
                        size_of::<WAVEHDR>() as UINT,
                    );
                }

                ring_idx = (ring_idx + 1) % RING_SIZE;
            }

            let _ = waveOutReset(h_wave_out);
            for hdr in headers.iter_mut() {
                if hdr.dwFlags != 0 {
                    let _ = waveOutUnprepareHeader(
                        h_wave_out,
                        hdr as *mut WAVEHDR,
                        size_of::<WAVEHDR>() as UINT,
                    );
                }
            }
            let _ = waveOutClose(h_wave_out);
        });

        Self { tx, stop_flag }
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

impl Drop for StreamAudioPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}
