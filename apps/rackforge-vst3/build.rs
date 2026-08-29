use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=RACKFORGE_BUNDLED_PLUGIN");
    println!("cargo:rerun-if-env-changed=RACKFORGE_BUNDLED_OFFICIAL_PLUGINS");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("bundled_plugin.rs");
    let source = env::var_os("RACKFORGE_BUNDLED_PLUGIN")
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    let primary = match source {
        Some(path) => {
            println!("cargo:rerun-if-changed={}", path.display());
            format!(
                "pub static BUNDLED_PLUGIN: Option<&'static [u8]> = Some(include_bytes!(r#\"{}\"#));\n",
                path.display()
            )
        }
        None => "pub static BUNDLED_PLUGIN: Option<&'static [u8]> = None;\n".into(),
    };

    // Every official instrument travels inside the plug-in, the way the
    // desktop carries them: the directory is the list, so adding one
    // upstream needs no change here.
    let mut archives = env::var_os("RACKFORGE_BUNDLED_OFFICIAL_PLUGINS")
        .map(PathBuf::from)
        .map(|directory| {
            println!("cargo:rerun-if-changed={}", directory.display());
            let mut found = fs::read_dir(&directory)
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
            found.sort();
            found
        })
        .unwrap_or_default();
    archives.dedup();

    let mut official = String::from("pub static BUNDLED_OFFICIAL_PLUGINS: &[(&str, &[u8])] = &[\n");
    for archive in archives {
        let archive = fs::canonicalize(&archive).expect("bundled official plugin must be readable");
        let name = archive
            .file_stem()
            .and_then(|name| name.to_str())
            .expect("bundled official plugin filename must be UTF-8")
            .to_owned();
        println!("cargo:rerun-if-changed={}", archive.display());
        official.push_str(&format!(
            "    ({name:?}, include_bytes!(r#\"{}\"#)),\n",
            archive.display()
        ));
    }
    official.push_str("];\n");

    fs::write(output, format!("{primary}{official}")).expect("write bundled plugin source");
}
