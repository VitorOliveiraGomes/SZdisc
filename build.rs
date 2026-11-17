use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    embed_screenz_web_dist();
    println!("cargo:rerun-if-changed=app.manifest");
    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest")),
    );
    tauri_build::try_build(attributes).expect("falha ao preparar o aplicativo Tauri");
}

/// Embute `ScreenZWeb/dist` dentro do executável para que o app abra a interface
/// mesmo quando é executado sozinho (sem a pasta `ScreenZWeb/dist` ao lado).
/// Se o `dist` ainda não existir mas o `node_modules` sim, tenta gerar com `npm run build`.
fn embed_screenz_web_dist() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let screenz_web = manifest_dir.join("ScreenZWeb");
    let dist = screenz_web.join("dist");

    println!("cargo:rerun-if-changed=ScreenZWeb/dist");
    println!("cargo:rerun-if-changed=ScreenZWeb/src");
    println!("cargo:rerun-if-changed=ScreenZWeb/index.html");
    println!("cargo:rerun-if-changed=ScreenZWeb/package.json");
    println!("cargo:rerun-if-env-changed=SCREENZ_SKIP_WEB_BUILD");

    let dist_index = dist.join("index.html");
    let dist_mtime = fs::metadata(&dist_index).and_then(|m| m.modified()).ok();
    let src_newer = match dist_mtime {
        None => true,
        Some(dm) => {
            newest_mtime(&screenz_web.join("src")).is_some_and(|sm| sm > dm)
                || fs::metadata(screenz_web.join("index.html"))
                    .and_then(|m| m.modified())
                    .is_ok_and(|im| im > dm)
        }
    };

    if src_newer
        && screenz_web.join("node_modules").is_dir()
        && env::var_os("SCREENZ_SKIP_WEB_BUILD").is_none()
    {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        let ok = Command::new(npm)
            .args(["run", "build"])
            .current_dir(&screenz_web)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            println!("cargo:warning=Falha ao gerar ScreenZWeb/dist automaticamente (rode `cd ScreenZWeb && npm run build`)");
        }
    }

    let mut files = Vec::new();
    if dist.join("index.html").is_file() {
        collect_files(&dist, &dist, &mut files);
    } else {
        println!(
            "cargo:warning=ScreenZWeb/dist não encontrado: o executável não terá a interface embutida"
        );
    }
    files.sort();

    let mut code = String::from("pub static EMBEDDED_WEB_ASSETS: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in &files {
        code.push_str(&format!(
            "    ({:?}, include_bytes!({:?})),\n",
            rel,
            abs.display().to_string()
        ));
    }
    code.push_str("];\n");

    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("embedded_web_assets.rs");
    fs::write(out, code).expect("falha ao gravar embedded_web_assets.rs");
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.push((rel, path.clone()));
        }
    }
}

fn newest_mtime(dir: &Path) -> Option<std::time::SystemTime> {
    let entries = fs::read_dir(dir).ok()?;
    let mut newest: Option<std::time::SystemTime> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let cand = if path.is_dir() {
            newest_mtime(&path)
        } else {
            fs::metadata(&path).and_then(|m| m.modified()).ok()
        };
        if let Some(t) = cand {
            newest = Some(newest.map_or(t, |cur| cur.max(t)));
        }
    }
    newest
}
