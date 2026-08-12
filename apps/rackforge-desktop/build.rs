fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/rackforge.ico");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        let target_environment = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
        let toolkit = windows_resource_compiler_directory();
        if let Some(toolkit) = toolkit.as_ref() {
            resource.set_toolkit_path(toolkit.to_string_lossy().as_ref());
        }
        let compiler_available = target_environment == "msvc" && toolkit.is_some()
            || target_environment == "gnu"
                && std::process::Command::new("windres")
                    .arg("--version")
                    .output()
                    .is_ok();
        if !compiler_available {
            println!(
                "cargo:warning=Windows resource compiler unavailable for {target_environment}; \
                 the runtime window icon remains embedded, but this non-release executable will \
                 not have an Explorer icon"
            );
            return;
        }
        resource
            .set_icon("../../assets/brand/rackforge.ico")
            .set("ProductName", "RackForge")
            .set("FileDescription", "RackForge Desktop")
            .set("LegalCopyright", "RackForge contributors")
            .compile()
            .expect("embedding RackForge Windows resources");
    }
}

#[cfg(windows)]
fn windows_resource_compiler_directory() -> Option<std::path::PathBuf> {
    let architecture = if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86") {
        "x86"
    } else {
        "x64"
    };
    if let Some(versioned_bin) = std::env::var_os("WindowsSdkVerBinPath") {
        let directory = std::path::PathBuf::from(versioned_bin).join(architecture);
        if directory.join("rc.exe").is_file() {
            return Some(directory);
        }
    }

    let kits = std::env::var_os("ProgramFiles(x86)")
        .map(std::path::PathBuf::from)?
        .join("Windows Kits/10/bin");
    let mut versions = std::fs::read_dir(kits)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    versions.sort();
    versions.into_iter().rev().find_map(|version| {
        let directory = version.join(architecture);
        directory.join("rc.exe").is_file().then_some(directory)
    })
}
