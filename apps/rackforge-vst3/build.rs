use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=RACKFORGE_BUNDLED_PLUGIN");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("bundled_plugin.rs");
    let source = env::var_os("RACKFORGE_BUNDLED_PLUGIN")
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    let generated = match source {
        Some(path) => {
            println!("cargo:rerun-if-changed={}", path.display());
            format!(
                "pub static BUNDLED_PLUGIN: Option<&'static [u8]> = Some(include_bytes!(r#\"{}\"#));\n",
                path.display()
            )
        }
        None => "pub static BUNDLED_PLUGIN: Option<&'static [u8]> = None;\n".into(),
    };
    fs::write(output, generated).expect("write bundled plugin source");
}
