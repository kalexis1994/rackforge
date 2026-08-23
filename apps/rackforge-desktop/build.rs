fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/rackforge.ico");
    stamp_revision();
    generate_bundled_plugin_module();

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

fn generate_bundled_plugin_module() {
    println!("cargo:rerun-if-env-changed=RACKFORGE_BUNDLED_PLUGIN");
    println!("cargo:rerun-if-env-changed=RACKFORGE_BUNDLED_OFFICIAL_PLUGINS");
    println!("cargo:rerun-if-env-changed=RACKFORGE_BUNDLED_CONTROLLER_DRIVER");
    let output = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").expect("Cargo always defines OUT_DIR"),
    )
    .join("bundled_plugin.rs");
    let mut source = match std::env::var_os("RACKFORGE_BUNDLED_PLUGIN") {
        Some(path) => {
            let path = std::fs::canonicalize(path)
                .expect("RACKFORGE_BUNDLED_PLUGIN must name a readable package");
            println!("cargo:rerun-if-changed={}", path.display());
            format!(
                "const BUNDLED_DEFAULT_PLUGIN: Option<&[u8]> = Some(include_bytes!({path:?}));\n"
            )
        }
        None => "const BUNDLED_DEFAULT_PLUGIN: Option<&[u8]> = None;\n".into(),
    };
    let official_plugins = std::env::var_os("RACKFORGE_BUNDLED_OFFICIAL_PLUGINS")
        .map(std::path::PathBuf::from)
        .map(|directory| {
            println!("cargo:rerun-if-changed={}", directory.display());
            let mut archives = std::fs::read_dir(&directory)
                .unwrap_or_else(|error| {
                    panic!(
                        "RACKFORGE_BUNDLED_OFFICIAL_PLUGINS must name a readable directory: {error}"
                    )
                })
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("rfplugin"))
                })
                .collect::<Vec<_>>();
            archives.sort();
            archives
        })
        .unwrap_or_default();
    source.push_str("const BUNDLED_OFFICIAL_PLUGINS: &[(&str, &[u8])] = &[\n");
    for archive in official_plugins {
        let archive = std::fs::canonicalize(&archive)
            .expect("bundled official plugin must be a readable package");
        let name = archive
            .file_name()
            .and_then(|name| name.to_str())
            .expect("bundled official plugin filename must be UTF-8");
        println!("cargo:rerun-if-changed={}", archive.display());
        source.push_str(&format!("    ({name:?}, include_bytes!({archive:?})),\n"));
    }
    source.push_str("];\n");
    source.push_str(
        &match std::env::var_os("RACKFORGE_BUNDLED_CONTROLLER_DRIVER") {
            Some(path) => {
                let path = std::fs::canonicalize(path).expect(
                    "RACKFORGE_BUNDLED_CONTROLLER_DRIVER must name a readable driver",
                );
                println!("cargo:rerun-if-changed={}", path.display());
                format!(
                    "const BUNDLED_CONTROLLER_DRIVER: Option<&[u8]> = Some(include_bytes!({path:?}));\n"
                )
            }
            None => "const BUNDLED_CONTROLLER_DRIVER: Option<&[u8]> = None;\n".into(),
        },
    );
    std::fs::write(output, source).expect("writing bundled plugin module");
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

/// Stamps the git revision into the binary. In-repo builds ask git; trees
/// produced by `git archive` carry the hash substituted into REVISION via
/// export-subst (how the Raspberry Pi builds); anything else is dev.
fn stamp_revision() {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .or_else(|| {
            std::fs::read_to_string("../../REVISION")
                .ok()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.contains("Format"))
        })
        .unwrap_or_else(|| "dev".to_owned());
    println!("cargo:rustc-env=RACKFORGE_REVISION={revision}");
    println!("cargo:rerun-if-changed=../../REVISION");
}
