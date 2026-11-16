use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    embed_szweb_dist();

    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=app.rc");
        println!("cargo:rerun-if-changed=app.manifest");

        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let res_obj = out_dir.join("app_res.o");

        let status = Command::new("windres")
            .args(["app.rc", "-O", "coff", "-o"])
            .arg(&res_obj)
            .status();

        if let Ok(s) = status {
            if s.success() {
                println!("cargo:rustc-link-arg-bins={}", res_obj.display());
            }
        }
    }
}

/// Embute `SZweb/dist` dentro do executável para que o app abra a interface
/// mesmo quando é executado sozinho (sem a pasta `SZweb/dist` ao lado).
/// Se o `dist` ainda não existir mas o `node_modules` sim, tenta gerar com `npm run build`.
fn embed_szweb_dist() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let szweb = manifest_dir.join("SZweb");
    let dist = szweb.join("dist");

    println!("cargo:rerun-if-changed=SZweb/dist");
    println!("cargo:rerun-if-env-changed=SZDISC_SKIP_WEB_BUILD");

    if !dist.join("index.html").is_file()
        && szweb.join("node_modules").is_dir()
        && env::var_os("SZDISC_SKIP_WEB_BUILD").is_none()
    {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        let ok = Command::new(npm)
            .args(["run", "build"])
            .current_dir(&szweb)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            println!("cargo:warning=Falha ao gerar SZweb/dist automaticamente (rode `cd SZweb && npm run build`)");
        }
    }

    let mut files = Vec::new();
    if dist.join("index.html").is_file() {
        collect_files(&dist, &dist, &mut files);
    } else {
        println!("cargo:warning=SZweb/dist não encontrado: o executável não terá a interface embutida");
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
