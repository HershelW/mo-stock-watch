use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-env-changed=RC");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let icon_path = manifest_dir.join("assets").join("app.ico");
    if !icon_path.exists() {
        panic!("missing icon: {}", icon_path.display());
    }

    let resource_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::create_dir_all(&resource_dir).expect("create resource dir");
    let temp_icon_path = resource_dir.join("app.ico");
    let rc_path = resource_dir.join("app.rc");
    let res_path = resource_dir.join("app.res");
    fs::copy(&icon_path, &temp_icon_path).expect("copy icon to temp resource dir");

    fs::write(&rc_path, "1 ICON \"app.ico\"\n").expect("write icon rc");

    let rc = find_rc().unwrap_or_else(|| PathBuf::from("rc.exe"));
    let status = Command::new(&rc)
        .current_dir(&resource_dir)
        .arg("/nologo")
        .arg("/fo")
        .arg("app.res")
        .arg("app.rc")
        .status()
        .expect("run Windows resource compiler");

    if !status.success() {
        panic!("resource compiler failed: {}", rc.display());
    }
    if !res_path.exists() {
        panic!("resource compiler did not create {}", res_path.display());
    }

    println!(
        "cargo:rustc-link-arg-bin=mo-stock-watch={}",
        res_path.display()
    );
}

fn find_rc() -> Option<PathBuf> {
    if let Some(path) = env::var_os("RC")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    let mut candidates = Vec::new();
    if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
        let kits_bin = PathBuf::from(program_files_x86)
            .join("Windows Kits")
            .join("10")
            .join("bin");
        if let Ok(entries) = fs::read_dir(kits_bin) {
            for entry in entries.flatten() {
                let rc = entry.path().join("x64").join("rc.exe");
                if rc.exists() {
                    candidates.push(rc);
                }
            }
        }
    }

    candidates.sort();
    candidates.pop()
}
