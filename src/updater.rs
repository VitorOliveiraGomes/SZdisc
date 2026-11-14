use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

pub const CURRENT_VERSION: &str = "2.2.0";
const VERSION_URL: &str = "https://raw.githubusercontent.com/VitorOliveiraGomes/SZdisc/main/VERSION.txt";
const EXE_DOWNLOAD_URL: &str = "https://github.com/VitorOliveiraGomes/SZdisc/raw/main/DISC2.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable(String),
    Downloading,
    Failed(String),
}

/// Remove arquivo antigo `.old.exe` deixado pela atualização anterior ao iniciar o aplicativo
pub fn cleanup_old_exe() {
    if let Ok(curr) = std::env::current_exe() {
        let old_path = curr.with_extension("old.exe");
        let _ = fs::remove_file(old_path);
    }
    let _ = fs::remove_file("DISC2.old.exe");
}

/// Verifica em segundo plano se há uma versão mais recente no repositório GitHub
pub fn check_for_updates_async(status: Arc<Mutex<UpdateStatus>>) {
    if let Ok(mut s) = status.lock() {
        *s = UpdateStatus::Checking;
    }

    thread::spawn(move || {
        let output = Command::new("curl.exe")
            .args([
                "-s",
                "-L",
                "--max-time",
                "6",
                "-H",
                "Cache-Control: no-cache",
                VERSION_URL,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let remote_ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !remote_ver.is_empty() && remote_ver.len() <= 16 {
                    if let Ok(mut s) = status.lock() {
                        if remote_ver != CURRENT_VERSION {
                            *s = UpdateStatus::UpdateAvailable(remote_ver);
                        } else {
                            *s = UpdateStatus::UpToDate;
                        }
                    }
                } else if let Ok(mut s) = status.lock() {
                    *s = UpdateStatus::UpToDate;
                }
            }
            _ => {
                if let Ok(mut s) = status.lock() {
                    *s = UpdateStatus::Idle;
                }
            }
        }
    });
}

/// Baixa o `DISC2.exe` mais recente direto do GitHub, substitui o executável atual e reinicia sozinho
pub fn download_and_apply_update_async(status: Arc<Mutex<UpdateStatus>>) {
    if let Ok(mut s) = status.lock() {
        *s = UpdateStatus::Downloading;
    }

    thread::spawn(move || {
        let current_exe: PathBuf = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                if let Ok(mut s) = status.lock() {
                    *s = UpdateStatus::Failed(format!("Erro ao localizar exe: {}", e));
                }
                return;
            }
        };

        let temp_new = current_exe.with_extension("new.exe");
        let temp_old = current_exe.with_extension("old.exe");
        let temp_new_str = temp_new.to_string_lossy().to_string();

        let status_cmd = Command::new("curl.exe")
            .args([
                "-s",
                "-L",
                "--fail",
                "--max-time",
                "60",
                "-H",
                "Cache-Control: no-cache",
                "-o",
                &temp_new_str,
                EXE_DOWNLOAD_URL,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status();

        match status_cmd {
            Ok(st) if st.success() => {
                let valid_size = fs::metadata(&temp_new)
                    .map(|m| m.len() > 1_000_000)
                    .unwrap_or(false);

                if !valid_size {
                    let _ = fs::remove_file(&temp_new);
                    if let Ok(mut s) = status.lock() {
                        *s = UpdateStatus::Failed("Arquivo baixado inválido.".to_string());
                    }
                    return;
                }

                let _ = fs::remove_file(&temp_old);
                if let Err(e) = fs::rename(&current_exe, &temp_old) {
                    let _ = fs::remove_file(&temp_new);
                    if let Ok(mut s) = status.lock() {
                        *s = UpdateStatus::Failed(format!("Erro ao renomear exe: {}", e));
                    }
                    return;
                }

                if let Err(e) = fs::rename(&temp_new, &current_exe) {
                    let _ = fs::rename(&temp_old, &current_exe);
                    if let Ok(mut s) = status.lock() {
                        *s = UpdateStatus::Failed(format!("Erro ao aplicar exe: {}", e));
                    }
                    return;
                }

                // Abre o novo executável atualizado e encerra a versão antiga
                let _ = Command::new(&current_exe).spawn();
                std::process::exit(0);
            }
            _ => {
                let _ = fs::remove_file(&temp_new);
                if let Ok(mut s) = status.lock() {
                    *s = UpdateStatus::Failed("Falha ao baixar do GitHub.".to_string());
                }
            }
        }
    });
}
