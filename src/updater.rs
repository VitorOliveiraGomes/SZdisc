use std::ffi::CString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};
use std::thread;

use winapi::shared::minwindef::DWORD;
use winapi::um::wininet::{
    InternetCloseHandle, InternetOpenA, InternetOpenUrlA, InternetReadFile,
    INTERNET_FLAG_NO_CACHE_WRITE, INTERNET_FLAG_RELOAD, INTERNET_OPEN_TYPE_PRECONFIG,
};

pub const CURRENT_VERSION: &str = "2.9.1";
const VERSION_URL: &str =
    "https://raw.githubusercontent.com/VitorOliveiraGomes/SZdisc/main/VERSION.txt";
const EXE_DOWNLOAD_URL: &str =
    "https://github.com/VitorOliveiraGomes/SZdisc/raw/main/DISC2.exe";

#[derive(Clone, Debug)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable(String),
    Downloading,
    Failed,
}

/// Faz download HTTPS usando a API oficial nativa do Windows (wininet.dll),
/// sem abrir processos ocultos como curl.exe ou powershell.exe (evita falsos positivos de antivírus).
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
    if let Ok(mut s) = status.lock() {
        *s = UpdateStatus::Checking;
    }

    thread::spawn(move || {
        if let Some(bytes) = fetch_https_bytes_wininet(VERSION_URL, 256) {
            let remote_ver = String::from_utf8_lossy(&bytes).trim().to_string();
            if !remote_ver.is_empty()
                && remote_ver.len() <= 16
                && remote_ver.chars().all(|c| c.is_ascii_digit() || c == '.')
            {
                if let Ok(mut s) = status.lock() {
                    if remote_ver != CURRENT_VERSION {
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
    if let Ok(mut s) = status.lock() {
        *s = UpdateStatus::Downloading;
    }

    thread::spawn(move || {
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

        let Some(exe_bytes) = fetch_https_bytes_wininet(EXE_DOWNLOAD_URL, 45_000_000) else {
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
