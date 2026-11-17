use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::AUDIO_SAMPLE_RATE;

pub fn spawn_system_audio_loopback<F>(
    _stop_flag: Arc<AtomicBool>,
    _audio_enabled: Arc<AtomicBool>,
    _target_pid: Arc<AtomicU32>,
    _on_pcm_chunk: F,
) where
    F: FnMut(&[u8]) + Send + 'static,
{
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

        thread::spawn(move || {
            let host = cpal::default_host();
            let Some(device) = host.default_output_device() else {
                return;
            };
            let Ok(default_cfg) = device.default_output_config() else {
                return;
            };
            let out_rate = default_cfg.sample_rate().0;
            let out_channels = default_cfg.channels() as usize;
            let cfg = cpal::StreamConfig {
                channels: default_cfg.channels(),
                sample_rate: default_cfg.sample_rate(),
                buffer_size: cpal::BufferSize::Default,
            };

            let sample_queue: Arc<Mutex<VecDeque<f32>>> =
                Arc::new(Mutex::new(VecDeque::with_capacity(48_000)));
            let q_reader = Arc::clone(&sample_queue);
            let muted_c = Arc::clone(&muted);
            let vol_c = Arc::clone(&volume_percent);

            let stream = device.build_output_stream(
                &cfg,
                move |data: &mut [f32], _| {
                    let is_muted = muted_c.load(Ordering::Relaxed);
                    let vol = vol_c.load(Ordering::Relaxed) as f32 / 100.0;
                    if let Ok(mut q) = q_reader.lock() {
                        for frame in data.chunks_mut(out_channels) {
                            let l = q.pop_front().unwrap_or(0.0);
                            let r = q.pop_front().unwrap_or(l);
                            if is_muted {
                                for s in frame.iter_mut() {
                                    *s = 0.0;
                                }
                            } else {
                                for (ch, s) in frame.iter_mut().enumerate() {
                                    let v = if ch % 2 == 0 { l } else { r };
                                    *s = (v * vol).clamp(-1.0, 1.0);
                                }
                            }
                        }
                    }
                },
                |_| {},
                None,
            );

            let Ok(stream) = stream else {
                return;
            };
            let _ = stream.play();

            let ratio = out_rate as f32 / AUDIO_SAMPLE_RATE as f32;
            while !stop_clone.load(Ordering::Relaxed) {
                let Ok(pcm) = rx.recv_timeout(Duration::from_millis(200)) else {
                    continue;
                };
                if muted.load(Ordering::Relaxed) {
                    continue;
                }
                let frames_in = pcm.len() / 4;
                if frames_in == 0 {
                    continue;
                }
                if let Ok(mut q) = sample_queue.lock() {
                    if q.len() > 24_000 {
                        q.clear();
                    }
                    if (ratio - 1.0).abs() < 0.01 {
                        for chunk in pcm.chunks_exact(4) {
                            let l = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
                            let r = i16::from_le_bytes([chunk[2], chunk[3]]) as f32 / 32768.0;
                            q.push_back(l);
                            q.push_back(r);
                        }
                    } else {
                        let frames_out = ((frames_in as f32) * ratio) as usize;
                        for i in 0..frames_out {
                            let src_idx = ((i as f32 / ratio) as usize).min(frames_in - 1);
                            let off = src_idx * 4;
                            let l = i16::from_le_bytes([pcm[off], pcm[off + 1]]) as f32 / 32768.0;
                            let r =
                                i16::from_le_bytes([pcm[off + 2], pcm[off + 3]]) as f32 / 32768.0;
                            q.push_back(l);
                            q.push_back(r);
                        }
                    }
                }
            }
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
