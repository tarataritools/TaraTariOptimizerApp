fn main() {
    #[cfg(windows)]
    {
        if std::env::var_os("CARGO_FEATURE_ADMIN_ELEVATION").is_some() {
            let mut resource = winres::WindowsResource::new();
            resource.set_manifest_file("app.manifest");
            resource.set_icon("assets/TaraTariOptimizerAppLogo.ico");
            resource
                .compile()
                .expect("failed to embed the Windows administrator manifest");
            publish_hardware_bridge();
        }
    }
    println!("cargo:rerun-if-changed=assets/TaraTariOptimizerAppLogo.ico");
    println!("cargo:rerun-if-changed=assets/TaraTariOptimizerAppLogoCircle.png");
    println!("cargo:rerun-if-changed=hardware-bridge/Program.cs");
    println!("cargo:rerun-if-changed=hardware-bridge/TaraTariHardwareBridge.csproj");
    println!("cargo:rerun-if-env-changed=PATH");
    println!("cargo:rerun-if-env-changed=DOTNET_ROOT");
}

#[cfg(windows)]
fn publish_hardware_bridge() {
    use std::{env, path::PathBuf, process::Command};

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let Some(project) = ["hardware-bridge/TaraTariHardwareBridge.csproj"]
        .iter()
        .map(|path| manifest_dir.join(path))
        .find(|path| path.is_file())
    else {
        return;
    };
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                manifest_dir.join(path)
            }
        })
        .unwrap_or_else(|| manifest_dir.join("target"));
    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    let target_dir = if target != host {
        target_dir.join(&target)
    } else {
        target_dir
    };
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_owned());
    let runtime = if target.starts_with("aarch64-") {
        "win-arm64"
    } else if target.starts_with("i686-") {
        "win-x86"
    } else if target.starts_with("x86_64-") {
        "win-x64"
    } else {
        println!("cargo:warning=Unsupported target for the LibreHardwareMonitor bridge; hardware sensor readings will be unavailable.");
        return;
    };
    let output = target_dir.join(profile).join("hardware-bridge");
    let dotnet = env::var_os("DOTNET_ROOT")
        .map(PathBuf::from)
        .map(|path| {
            path.join(if cfg!(windows) {
                "dotnet.exe"
            } else {
                "dotnet"
            })
        })
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("dotnet"));
    if !matches!(
        Command::new(&dotnet).arg("--version").output(),
        Ok(result) if result.status.success()
    ) {
        println!("cargo:warning=.NET SDK not found; LibreHardwareMonitor bridge was not built and hardware readings will be unavailable.");
        return;
    }
    let result = Command::new(dotnet)
        .arg("publish")
        .arg(project)
        .args(["--configuration", "Release", "--runtime", runtime])
        .args([
            "--self-contained",
            "true",
            "-p:PublishSingleFile=true",
            "-p:IncludeNativeLibrariesForSelfExtract=true",
        ])
        .arg("--output")
        .arg(&output)
        .status()
        .expect("failed to start dotnet for LibreHardwareMonitor bridge");
    if !result.success() {
        panic!("LibreHardwareMonitor bridge publish failed");
    }
}
