#[cfg(windows)]
mod windows_capture;
#[cfg(windows)]
mod windows_playback;

#[cfg(not(windows))]
mod unix;

pub const AUDIO_SAMPLE_RATE: u32 = 24_000;
pub const AUDIO_CHANNELS: u16 = 2;
pub const AUDIO_BITS_PER_SAMPLE: u16 = 16;

#[cfg(windows)]
pub use windows_capture::spawn_system_audio_loopback;
#[cfg(windows)]
pub use windows_playback::StreamAudioPlayer;

#[cfg(not(windows))]
pub use unix::{spawn_system_audio_loopback, StreamAudioPlayer};
