use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{BYTE, DWORD, UINT, WORD};
use winapi::shared::mmreg::{WAVEFORMATEX, WAVE_FORMAT_PCM};
use winapi::shared::winerror::S_OK;
use winapi::um::audioclient::{
    IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK,
};
use winapi::um::combaseapi::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL};
use winapi::um::mmdeviceapi::{eConsole, eRender, IMMDevice, IMMDeviceEnumerator};
use winapi::um::mmsystem::{
    waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader,
    waveOutWrite, CALLBACK_NULL, HWAVEOUT, MMSYSERR_NOERROR, WAVEHDR, WAVE_MAPPER,
};
use winapi::um::objbase::COINIT_MULTITHREADED;

pub const AUDIO_SAMPLE_RATE: u32 = 24000;
pub const AUDIO_CHANNELS: u16 = 2;
pub const AUDIO_BITS_PER_SAMPLE: u16 = 16;

// Flags do Windows 10/11 para o próprio mixer converter automaticamente para o nosso formato PCM
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

/// Inicia a captura de áudio do sistema (WASAPI Loopback) em uma thread dedicada
pub fn spawn_system_audio_loopback<F>(
    stop_flag: Arc<AtomicBool>,
    audio_enabled: Arc<AtomicBool>,
    mut on_pcm_chunk: F,
) where
    F: FnMut(&[u8]) + Send + 'static,
{
    thread::spawn(move || unsafe {
        let _ = CoInitializeEx(null_mut(), COINIT_MULTITHREADED);

        let mut enumerator: *mut IMMDeviceEnumerator = null_mut();
        let hr = CoCreateInstance(
            &CLSID_MMDEVICE_ENUMERATOR,
            null_mut(),
            CLSCTX_ALL,
            &IID_IMMDEVICE_ENUMERATOR,
            &mut enumerator as *mut _ as *mut _,
        );
        if hr != S_OK || enumerator.is_null() {
            return;
        }

        let mut device: *mut IMMDevice = null_mut();
        let hr = (*enumerator).GetDefaultAudioEndpoint(eRender, eConsole, &mut device);
        if hr != S_OK || device.is_null() {
            (*enumerator).Release();
            return;
        }

        let mut audio_client: *mut IAudioClient = null_mut();
        let hr = (*device).Activate(
            &IID_IAUDIO_CLIENT,
            CLSCTX_ALL,
            null_mut(),
            &mut audio_client as *mut _ as *mut _,
        );
        if hr != S_OK || audio_client.is_null() {
            (*device).Release();
            (*enumerator).Release();
            return;
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

        // 50ms buffer
        let hns_requested_duration: i64 = 500_000;
        let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;

        let hr = (*audio_client).Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            flags,
            hns_requested_duration,
            0,
            &wfx,
            null_mut(),
        );
        if hr != S_OK {
            (*audio_client).Release();
            (*device).Release();
            (*enumerator).Release();
            return;
        }

        let mut capture_client: *mut IAudioCaptureClient = null_mut();
        let hr = (*audio_client).GetService(
            &IID_IAUDIO_CAPTURE_CLIENT,
            &mut capture_client as *mut _ as *mut _,
        );
        if hr != S_OK || capture_client.is_null() {
            (*audio_client).Release();
            (*device).Release();
            (*enumerator).Release();
            return;
        }

        if (*audio_client).Start() != S_OK {
            (*capture_client).Release();
            (*audio_client).Release();
            (*device).Release();
            (*enumerator).Release();
            return;
        }

        let mut accum_buf: Vec<u8> = Vec::with_capacity(4800);

        while !stop_flag.load(Ordering::Relaxed) {
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

                    // Envia blocos de ~20ms (1920 bytes a 24kHz estéreo 16-bit)
                    if accum_buf.len() >= 1920 {
                        // Verifica se o bloco não é 100% silêncio para economizar banda
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
        (*device).Release();
        (*enumerator).Release();
    });
}

/// Reprodutor de áudio de baixa latência para quem está assistindo à transmissão
pub struct StreamAudioPlayer {
    tx: SyncSender<Vec<u8>>,
    pub volume_percent: Arc<AtomicU32>,
    pub muted: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
}

impl StreamAudioPlayer {
    pub fn start(volume_percent: Arc<AtomicU32>, muted: Arc<AtomicBool>) -> Self {
        let (tx, rx) = sync_channel::<Vec<u8>>(16);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let vol_clone = Arc::clone(&volume_percent);
        let mute_clone = Arc::clone(&muted);

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

                if mute_clone.load(Ordering::Relaxed) {
                    continue;
                }

                let vol = vol_clone.load(Ordering::Relaxed) as f32 / 100.0;
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
                    // WHDR_DONE = 1; limpa cabeçalho anterior
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

        Self {
            tx,
            volume_percent,
            muted,
            stop_flag,
        }
    }

    pub fn push_pcm(&self, pcm_bytes: Vec<u8>) {
        let _ = self.tx.try_send(pcm_bytes);
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
