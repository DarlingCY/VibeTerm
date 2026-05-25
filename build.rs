use std::{fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=capabilities/default.json");
    println!("cargo:rerun-if-changed=ui/index.html");
    println!("cargo:rerun-if-changed=assets/xterm");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.png");
    println!("cargo:rerun-if-changed=assets/icon.svg");

    copy_xterm_assets();
    tauri_build::build()
}

fn copy_xterm_assets() {
    let source_dir = Path::new("assets").join("xterm");
    let target_dir = Path::new("ui").join("vendor").join("xterm");

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).expect("failed to clear ui/vendor/xterm");
    }
    fs::create_dir_all(&target_dir).expect("failed to create ui/vendor/xterm");

    for entry in fs::read_dir(&source_dir).expect("failed to read assets/xterm") {
        let entry = entry.expect("failed to read xterm entry");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        println!("cargo:rerun-if-changed={}", path.display());
        fs::copy(&path, target_dir.join(entry.file_name())).expect("failed to copy xterm asset");
    }
}
