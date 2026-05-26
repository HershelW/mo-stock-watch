use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

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

    let temp_dir = env::temp_dir().join(format!("mo-stock-watch-resource-{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("create temp resource dir");
    let temp_icon_path = temp_dir.join("app.ico");
    let rc_path = temp_dir.join("app.rc");
    let res_path = temp_dir.join("app.res");
    fs::copy(&icon_path, &temp_icon_path).expect("copy icon to temp resource dir");

    let icon_path = escape_rc_path(&temp_icon_path);
    fs::write(&rc_path, format!("1 ICON \"{icon_path}\"\n")).expect("write icon rc");

    let rc = find_rc().unwrap_or_else(|| PathBuf::from("rc.exe"));
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_path)
        .status()
        .expect("run Windows resource compiler");

    if !status.success() {
        panic!("resource compiler failed: {}", rc.display());
    }

    println!(
        "cargo:rustc-link-arg-bin=mo-stock-watch={}",
        res_path.display()
    );
}

fn escape_rc_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
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
