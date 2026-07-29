use anyhow::{Context, Result, bail};
use artupy_plugin_api::PluginManifest;
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "artupy-plugin.toml";

#[derive(Clone, Debug)]
pub struct PluginPackage {
    root: PathBuf,
    manifest: PluginManifest,
}

impl PluginPackage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let requested = path.as_ref();
        let manifest_path = if requested.is_dir() {
            requested.join(MANIFEST_FILE)
        } else {
            requested.to_path_buf()
        };
        let text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest: PluginManifest = toml::from_str(&text)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        manifest
            .validate()
            .with_context(|| format!("validating {}", manifest_path.display()))?;
        let root = manifest_path
            .parent()
            .context("plugin manifest has no parent directory")?
            .to_path_buf();
        Ok(Self { root, manifest })
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn binary_path(&self) -> Result<PathBuf> {
        let platform = platform_key()?;
        let relative = self.manifest.binary_for(platform)?;
        Ok(self.root.join(relative))
    }
}

pub fn platform_key() -> Result<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Ok("linux-aarch64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("linux-x86_64")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("windows-x86_64")
    } else {
        bail!(
            "unsupported ArtuPy plugin platform {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_the_development_platform() {
        let platform = platform_key().unwrap();
        assert!(platform.contains('-'));
    }
}
