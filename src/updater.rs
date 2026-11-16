use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::ffi::CString;
#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use winapi::shared::minwindef::DWORD;
#[cfg(windows)]
use winapi::um::wininet::{
    InternetCloseHandle, InternetOpenA, InternetOpenUrlA, InternetReadFile,
    INTERNET_FLAG_NO_CACHE_WRITE, INTERNET_FLAG_RELOAD, INTERNET_OPEN_TYPE_PRECONFIG,
};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const VERSION_URL: &str =
    "https://raw.githubusercontent.com/VitorOliveiraGomes/SZdisc/main/VERSION.txt";
const EXE_DOWNLOAD_URL: &str =
    "https://github.com/VitorOliveiraGomes/SZdisc/raw/main/DISC2.exe";

fn parse_semver(ver: &str) -> (u32, u32, u32) {
    let mut parts = ver.trim().split('.');
    let maj = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let min = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let pat = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (maj, min, pat)
}

fn is_remote_newer(remote: &str, current: &str) -> bool {
    parse_semver(remote) > parse_semver(current)
}

fn cache_busted_url(base: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}?t={}", base, ts)
}

#[derive(Clone, Debug)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable(String),
    Downloading,
    Failed,
}

#[cfg(windows)]
fn fetch_https_bytes_wininet(url: &str, max_bytes: usize) -> Option<Vec<u8>> {
    unsafe {
        let agent = CString::new("SZdisc-Desktop/2.5 (Windows NT 10.0; Win64; x64)").ok()?;
        let c_url = CString::new(url).ok()?;

        let h_inet = InternetOpenA(
            agent.as_ptr(),
            INTERNET_OPEN_TYPE_PRECONFIG,
            null_mut(),
            null_mut(),
            0,
        );
        if h_inet.is_null() {
            return None;
        }

        let flags = INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_CACHE_WRITE;
        let h_url = InternetOpenUrlA(h_inet, c_url.as_ptr(), null_mut(), 0, flags, 0);
        if h_url.is_null() {
            InternetCloseHandle(h_inet);
            return None;
        }

        let mut out = Vec::new();
        let mut buf = [0u8; 16384];
        loop {
            let mut bytes_read: DWORD = 0;
            let ok = InternetReadFile(
                h_url,
                buf.as_mut_ptr() as *mut _,
                buf.len() as DWORD,
                &mut bytes_read,
            );
            if ok == 0 || bytes_read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..bytes_read as usize]);
            if out.len() >= max_bytes {
                break;
            }
        }

        InternetCloseHandle(h_url);
        InternetCloseHandle(h_inet);

        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

#[cfg(not(windows))]
fn fetch_https_bytes_wininet(url: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "20", url])
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() || out.stdout.len() > max_bytes {
        None
    } else {
        Some(out.stdout)
    }
}

/// Remove arquivo antigo `.old.exe` deixado pela atualização anterior ao iniciar o aplicativo
pub fn cleanup_old_exe() {
    if let Ok(curr) = std::env::current_exe() {
        let old_path = curr.with_extension("old.exe");
        let _ = fs::remove_file(old_path);
    }
    let _ = fs::remove_file("DISC2.old.exe");
}

/// Verifica em segundo plano se há uma versão mais recente no repositório GitHub usando WinInet nativo
pub fn check_for_updates_async(status: Arc<Mutex<UpdateStatus>>) {
    if cfg!(debug_assertions) || std::env::var_os("SZDISC_DISABLE_UPDATER").is_some() {
        if let Ok(mut s) = status.lock() {
            *s = UpdateStatus::UpToDate;
        }
        return;
    }

    if let Ok(mut s) = status.lock() {
        *s = UpdateStatus::Checking;
    }

    thread::spawn(move || {
        let ver_url = cache_busted_url(VERSION_URL);
        if let Some(bytes) = fetch_https_bytes_wininet(&ver_url, 256) {
            let remote_ver = String::from_utf8_lossy(&bytes).trim().to_string();
            if !remote_ver.is_empty()
                && remote_ver.len() <= 16
                && remote_ver.chars().all(|c| c.is_ascii_digit() || c == '.')
            {
                if let Ok(mut s) = status.lock() {
                    if is_remote_newer(&remote_ver, CURRENT_VERSION) {
                        *s = UpdateStatus::UpdateAvailable(remote_ver);
                    } else {
                        *s = UpdateStatus::UpToDate;
                    }
                }
                return;
            }
        }

        if let Ok(mut s) = status.lock() {
            *s = UpdateStatus::Idle;
        }
    });
}

/// Baixa o `DISC2.exe` mais recente direto do GitHub via WinInet nativo e reinicia o aplicativo
pub fn download_and_apply_update_async(status: Arc<Mutex<UpdateStatus>>) {
    if cfg!(debug_assertions) || std::env::var_os("SZDISC_DISABLE_UPDATER").is_some() {
        if let Ok(mut s) = status.lock() {
            *s = UpdateStatus::UpToDate;
        }
        return;
    }

    if let Ok(mut s) = status.lock() {
        *s = UpdateStatus::Downloading;
    }

    thread::spawn(move || {
        // Confirma primeiro se realmente existe versão mais nova no GitHub (nunca faz downgrade nem baixa às cegas!)
        let ver_url = cache_busted_url(VERSION_URL);
        let Some(bytes) = fetch_https_bytes_wininet(&ver_url, 256) else {
            if let Ok(mut s) = status.lock() {
                *s = UpdateStatus::Failed;
            }
            return;
        };
        let remote_ver = String::from_utf8_lossy(&bytes).trim().to_string();
        let valid_ver = !remote_ver.is_empty()
            && remote_ver.len() <= 16
            && remote_ver.chars().all(|c| c.is_ascii_digit() || c == '.');
        if !valid_ver {
            if let Ok(mut s) = status.lock() {
                *s = UpdateStatus::Failed;
            }
            return;
        }
        if !is_remote_newer(&remote_ver, CURRENT_VERSION) {
            if let Ok(mut s) = status.lock() {
                *s = UpdateStatus::UpToDate;
            }
            return;
        }

        let current_exe: PathBuf = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => {
                if let Ok(mut s) = status.lock() {
                    *s = UpdateStatus::Failed;
                }
                return;
            }
        };

        let temp_new = current_exe.with_extension("new.exe");
        let temp_old = current_exe.with_extension("old.exe");

        let exe_url = cache_busted_url(EXE_DOWNLOAD_URL);
        let Some(exe_bytes) = fetch_https_bytes_wininet(&exe_url, 45_000_000) else {
            if let Ok(mut s) = status.lock() {
                *s = UpdateStatus::Failed;
            }
            return;
        };

        // Verifica assinatura PE ("MZ") e tamanho mínimo para garantir que o binário é válido
        if exe_bytes.len() < 1_000_000 || &exe_bytes[0..2] != b"MZ" {
            if let Ok(mut s) = status.lock() {
                *s = UpdateStatus::Failed;
            }
            return;
        }

        if fs::write(&temp_new, &exe_bytes).is_err() {
            if let Ok(mut s) = status.lock() {
                *s = UpdateStatus::Failed;
            }
            return;
        }

        let _ = fs::remove_file(&temp_old);
        if fs::rename(&current_exe, &temp_old).is_err() {
            let _ = fs::remove_file(&temp_new);
            if let Ok(mut s) = status.lock() {
                *s = UpdateStatus::Failed;
            }
            return;
        }

        if fs::rename(&temp_new, &current_exe).is_err() {
            let _ = fs::rename(&temp_old, &current_exe);
            if let Ok(mut s) = status.lock() {
                *s = UpdateStatus::Failed;
            }
            return;
        }

        let _ = Command::new(&current_exe).spawn();
        std::process::exit(0);
    });
}
