use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use winapi::shared::guiddef::{GUID, REFIID};
use winapi::shared::minwindef::{BYTE, DWORD, LPVOID, UINT, ULONG, WORD};
use winapi::shared::mmreg::{WAVEFORMATEX, WAVE_FORMAT_PCM};
use winapi::shared::winerror::{E_NOINTERFACE, S_OK};
use winapi::um::audioclient::{IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_SILENT};
use winapi::um::audiosessiontypes::{AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK};
use winapi::um::combaseapi::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL};
use winapi::um::handleapi::CloseHandle;
use winapi::um::libloaderapi::{GetProcAddress, LoadLibraryA};
use winapi::um::mmdeviceapi::{eConsole, eRender, IMMDevice, IMMDeviceEnumerator};
use winapi::um::objbase::COINIT_MULTITHREADED;
use winapi::um::synchapi::{CreateEventW, SetEvent, WaitForSingleObject};
use winapi::um::unknwnbase::{IUnknown, IUnknownVtbl};
use winapi::um::winbase::WAIT_OBJECT_0;
use winapi::um::winnt::HRESULT;

use super::{AUDIO_BITS_PER_SAMPLE, AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};

const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: DWORD = 0x8000_0000;
const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: DWORD = 0x0800_0000;

const CLSID_MMDEVICE_ENUMERATOR: GUID = GUID {
    Data1: 0xBCDE_0395,
    Data2: 0xE52F,
    Data3: 0x467C,
    Data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
};

const IID_IMMDEVICE_ENUMERATOR: GUID = GUID {
    Data1: 0xA956_64D2,
    Data2: 0x9614,
    Data3: 0x4F35,
    Data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
};

const IID_IAUDIO_CLIENT: GUID = GUID {
    Data1: 0x1CB9_AD4C,
    Data2: 0xDBFA,
    Data3: 0x4C32,
    Data4: [0xB1, 0x78, 0xC2, 0xF5, 0x68, 0xA7, 0x03, 0xB2],
};

const IID_IAUDIO_CAPTURE_CLIENT: GUID = GUID {
    Data1: 0xC8AD_BD64,
    Data2: 0xE71E,
    Data3: 0x48A0,
    Data4: [0xA4, 0xDE, 0x18, 0x5C, 0x39, 0x5C, 0xD3, 0x17],
};

const IID_IUNKNOWN: GUID = GUID {
    Data1: 0x0000_0000,
    Data2: 0x0000,
    Data3: 0x0000,
    Data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const IID_IACTIVATE_AUDIO_INTERFACE_COMPLETION_HANDLER: GUID = GUID {
    Data1: 0x41D9_49AB,
    Data2: 0x9862,
    Data3: 0x444A,
    Data4: [0x80, 0xF6, 0xC2, 0x61, 0x33, 0x4D, 0xA5, 0xEB],
};

const IID_IAGILE_OBJECT: GUID = GUID {
    Data1: 0x94EA_2B94,
    Data2: 0xE9CC,
    Data3: 0x49E0,
    Data4: [0xC0, 0xFF, 0xEE, 0x64, 0xCA, 0x8F, 0x5B, 0x90],
};

#[repr(C)]
struct AudioClientProcessLoopbackParams {
    target_process_id: DWORD,
    process_loopback_mode: u32,
}

#[repr(C)]
struct AudioClientActivationParams {
    activation_type: u32,
    process_loopback_params: AudioClientActivationParamsInner,
}

type AudioClientActivationParamsInner = AudioClientProcessLoopbackParams;

#[repr(C)]
struct BlobData {
    cb_size: ULONG,
    p_blob_data: *mut BYTE,
}

#[repr(C)]
struct PropVariantBlob {
    vt: u16,
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

/// Alocado no heap (`Box::into_raw`) e liberado apenas quando `ref_count` chega a zero.
#[repr(C)]
struct CompletionHandlerCom {
    lp_vtbl: *const IActivateAudioInterfaceCompletionHandlerVtbl,
    ref_count: AtomicU32,
    h_event: LPVOID,
}

impl Drop for CompletionHandlerCom {
    fn drop(&mut self) {
        if !self.h_event.is_null() {
            unsafe {
                CloseHandle(self.h_event);
            }
            self.h_event = null_mut();
        }
    }
}

unsafe extern "system" fn handler_query_interface(
    this: *mut IUnknown,
    riid: REFIID,
    ppv: *mut LPVOID,
) -> HRESULT {
    if ppv.is_null() || riid.is_null() {
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
    let ptr = this as *mut CompletionHandlerCom;
    let prev = (*ptr).ref_count.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(ptr));
        0
    } else {
        prev - 1
    }
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

fn resolve_activate_audio_interface_async() -> Option<ActivateAudioInterfaceAsyncFn> {
    static PROC: OnceLock<Option<usize>> = OnceLock::new();
    let addr = *PROC.get_or_init(|| unsafe {
        let h_mod = LoadLibraryA(c"Mmdevapi.dll".as_ptr());
        if h_mod.is_null() {
            return None;
        }
        let proc = GetProcAddress(h_mod, c"ActivateAudioInterfaceAsync".as_ptr());
        if proc.is_null() {
            None
        } else {
            Some(proc as usize)
        }
    });
    addr.map(|p| unsafe { std::mem::transmute::<usize, ActivateAudioInterfaceAsyncFn>(p) })
}

unsafe fn activate_process_specific_audio_client(target_pid: u32) -> *mut IAudioClient {
    let Some(activate_fn) = resolve_activate_audio_interface_async() else {
        return null_mut();
    };

    let h_event = CreateEventW(null_mut(), 1, 0, null_mut());
    if h_event.is_null() {
        return null_mut();
    }

    let handler_ptr = Box::into_raw(Box::new(CompletionHandlerCom {
        lp_vtbl: &COMPLETION_HANDLER_VTBL,
        ref_count: AtomicU32::new(1),
        h_event,
    }));

    let mut act_params = AudioClientActivationParams {
        activation_type: 1,
        process_loopback_params: AudioClientProcessLoopbackParams {
            target_process_id: target_pid,
            process_loopback_mode: 0,
        },
    };

    let prop_var = PropVariantBlob {
        vt: 65,
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
        handler_ptr,
        &mut op,
    );

    if hr != S_OK || op.is_null() {
        if !op.is_null() {
            ((*(*op).lp_vtbl).parent.Release)(op as *mut IUnknown);
        }
        handler_release(handler_ptr as *mut IUnknown);
        return null_mut();
    }

    let wait_res = WaitForSingleObject(h_event, 2000);
    if wait_res != WAIT_OBJECT_0 {
        ((*(*op).lp_vtbl).parent.Release)(op as *mut IUnknown);
        handler_release(handler_ptr as *mut IUnknown);
        return null_mut();
    }

    let mut activate_hr: HRESULT = -1;
    let mut unk: *mut IUnknown = null_mut();
    let hr2 = ((*(*op).lp_vtbl).get_activate_result)(op, &mut activate_hr, &mut unk);
    ((*(*op).lp_vtbl).parent.Release)(op as *mut IUnknown);
    handler_release(handler_ptr as *mut IUnknown);

    if hr2 == S_OK && activate_hr == S_OK && !unk.is_null() {
        unk as *mut IAudioClient
    } else {
        if !unk.is_null() {
            (*unk).Release();
        }
        null_mut()
    }
}

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

/// Captura o áudio isolado do aplicativo selecionado (`target_pid > 0`) ou do sistema (`target_pid == 0`).
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
        let mut backoff_ms = 300u64;

        while !stop_flag.load(Ordering::Relaxed) {
            let current_pid = target_pid.load(Ordering::Relaxed);

            let mut audio_client = if current_pid > 0 {
                activate_process_specific_audio_client(current_pid)
            } else {
                null_mut()
            };

            if audio_client.is_null() {
                audio_client = activate_system_loopback_audio_client();
            }

            if audio_client.is_null() {
                thread::sleep(Duration::from_millis(backoff_ms));
                backoff_ms = (backoff_ms * 2).min(1500);
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

            let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;

            if (*audio_client).Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                flags,
                500_000,
                0,
                &wfx,
                null_mut(),
            ) != S_OK
            {
                (*audio_client).Release();
                thread::sleep(Duration::from_millis(backoff_ms));
                backoff_ms = (backoff_ms * 2).min(1500);
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
                thread::sleep(Duration::from_millis(backoff_ms));
                backoff_ms = (backoff_ms * 2).min(1500);
                continue;
            }

            if (*audio_client).Start() != S_OK {
                (*capture_client).Release();
                (*audio_client).Release();
                thread::sleep(Duration::from_millis(backoff_ms));
                backoff_ms = (backoff_ms * 2).min(1500);
                continue;
            }

            backoff_ms = 300;
            let mut accum_buf: Vec<u8> = Vec::with_capacity(4800);
            let mut silence_tail: u8 = 0;

            while !stop_flag.load(Ordering::Relaxed)
                && target_pid.load(Ordering::Relaxed) == current_pid
            {
                if !audio_enabled.load(Ordering::Relaxed) {
                    accum_buf.clear();
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                thread::sleep(Duration::from_millis(12));

                let mut packet_length: UINT = 0;
                if (*capture_client).GetNextPacketSize(&mut packet_length) != S_OK {
                    backoff_ms = (backoff_ms * 2).min(1500);
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
                                silence_tail = 3;
                                on_pcm_chunk(&accum_buf);
                            } else if silence_tail > 0 {
                                silence_tail -= 1;
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
            if !stop_flag.load(Ordering::Relaxed)
                && target_pid.load(Ordering::Relaxed) == current_pid
            {
                thread::sleep(Duration::from_millis(backoff_ms));
            }
        }
    });
}
