fn main() {
    #[cfg(target_os = "windows")]
    {
        use std::env;
        use std::path::PathBuf;
        use std::process::Command;

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
