use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use winapi::shared::minwindef::{DWORD, UINT, WORD};
use winapi::shared::mmreg::{WAVEFORMATEX, WAVE_FORMAT_PCM};
use winapi::um::mmeapi::{
    waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader,
    waveOutWrite,
};
use winapi::um::mmsystem::{CALLBACK_NULL, HWAVEOUT, MMSYSERR_NOERROR, WAVEHDR, WAVE_MAPPER};

use super::{AUDIO_BITS_PER_SAMPLE, AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};

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
            if waveOutOpen(&mut h_wave_out, WAVE_MAPPER, &wfx, 0, 0, CALLBACK_NULL)
                != MMSYSERR_NOERROR
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
                    for sample_bytes in pcm.as_chunks_mut::<2>().0 {
                        let s = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
                        let scaled = ((s as f32) * vol).clamp(-32768.0, 32767.0) as i16;
                        let out = scaled.to_le_bytes();
                        sample_bytes[0] = out[0];
                        sample_bytes[1] = out[1];
                    }
                }

                let mut chosen_idx = None;
                for offset in 0..RING_SIZE {
                    let idx = (ring_idx + offset) % RING_SIZE;
                    let flags = headers[idx].dwFlags;
                    if flags == 0 || (flags & 1) != 0 {
                        chosen_idx = Some(idx);
                        break;
                    }
                }
                let Some(slot_idx) = chosen_idx else {
                    continue;
                };

                let hdr = &mut headers[slot_idx];
                if (hdr.dwFlags & 1) != 0 {
                    let _ = waveOutUnprepareHeader(
                        h_wave_out,
                        hdr as *mut WAVEHDR,
                        size_of::<WAVEHDR>() as UINT,
                    );
                }

                buffers[slot_idx] = pcm;
                *hdr = zeroed();
                hdr.lpData = buffers[slot_idx].as_mut_ptr() as *mut i8;
                hdr.dwBufferLength = buffers[slot_idx].len() as DWORD;

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

                ring_idx = (slot_idx + 1) % RING_SIZE;
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

    pub fn clone_sender(&self) -> SyncSender<Vec<u8>> {
        self.tx.clone()
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
