use std::path::Path;

/// Tauri refuses to build when an `externalBin` is missing. In dev, use the helper from the Swift build
/// output when it exists, otherwise a placeholder that exits with an error (the app then runs without ASR).
fn ensure_helper() {
    let triple = std::env::var("TARGET").unwrap_or_default();
    let dest = Path::new("binaries").join(format!("openflow-helper-{triple}"));
    println!("cargo:rerun-if-changed=../sidecar/openflow-helper/.build/release/openflow-helper");
    let built = Path::new("../sidecar/openflow-helper/.build/release/openflow-helper");
    let _ = std::fs::create_dir_all("binaries");
    if built.exists() {
        let stale = match (std::fs::metadata(built), std::fs::metadata(&dest)) {
            (Ok(b), Ok(d)) => b.modified().ok() > d.modified().ok(),
            _ => true,
        };
        if stale {
            std::fs::copy(built, &dest).expect("copy openflow-helper");
        }
    } else if !dest.exists() {
        std::fs::write(&dest, "#!/bin/sh\necho 'openflow-helper not built: run scripts/build.sh' >&2\nexit 1\n")
            .expect("write helper placeholder");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
    }
}

fn main() {
    ensure_helper();
    tauri_build::build()
}
