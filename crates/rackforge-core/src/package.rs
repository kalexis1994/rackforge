use crate::PluginStorage;
use anyhow::{Context, Result, bail};
use rackforge_plugin_api::{PluginManifest, ResourceKind, validate_branding_assets};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "rackforge-plugin.toml";

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
        validate_branding_assets(&manifest, &root)
            .with_context(|| format!("validating branding in {}", root.display()))?;
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

    pub fn component_path(&self) -> Result<Option<PathBuf>> {
        let Some(component) = self.manifest.portable_component() else {
            return Ok(None);
        };
        Ok(Some(self.root.join(&component.path)))
    }

    pub fn resolve_resources(
        &self,
        overrides: &BTreeMap<String, PathBuf>,
        data_root: Option<&Path>,
    ) -> Result<BTreeMap<String, PathBuf>> {
        let declared: BTreeSet<_> = self
            .manifest
            .resources
            .iter()
            .map(|resource| resource.id.as_str())
            .collect();
        for id in overrides.keys() {
            if !declared.contains(id.as_str()) {
                bail!("plugin does not declare resource {id:?}");
            }
        }

        let plugin_data = data_root
            .map(|root| PluginStorage::new(root).ensure_plugin(&self.manifest.id))
            .transpose()?;
        let mut resolved = BTreeMap::new();
        for requirement in &self.manifest.resources {
            let source = if let Some(path) = overrides.get(&requirement.id) {
                Some((path.clone(), ResourceSource::Override))
            } else if let Some(relative) = requirement.package_path.as_deref() {
                let path = self.root.join(relative);
                path.exists().then_some((path, ResourceSource::Package))
            } else if let (Some(relative), Some(root)) =
                (requirement.data_path.as_deref(), plugin_data.as_ref())
            {
                let path = root.root.join(relative);
                path.exists().then_some((path, ResourceSource::PrivateData))
            } else {
                None
            };
            let Some((path, source)) = source else {
                if requirement.required {
                    bail!(
                        "plugin requires resource {:?} ({})",
                        requirement.id,
                        requirement.name
                    );
                }
                continue;
            };
            let canonical = fs::canonicalize(&path)
                .with_context(|| format!("resolving resource {}", path.display()))?;
            let confined_root = match source {
                ResourceSource::Override => None,
                ResourceSource::Package => Some(&self.root),
                ResourceSource::PrivateData => plugin_data.as_ref().map(|root| &root.root),
            };
            if let Some(root) = confined_root {
                let canonical_root = fs::canonicalize(root)
                    .with_context(|| format!("resolving resource root {}", root.display()))?;
                if !canonical.starts_with(&canonical_root) {
                    bail!(
                        "resource {:?} escaped {}",
                        requirement.id,
                        canonical_root.display()
                    );
                }
            }
            let valid_kind = match requirement.kind {
                ResourceKind::File => canonical.is_file(),
                ResourceKind::Directory => canonical.is_dir(),
            };
            if !valid_kind {
                bail!(
                    "resource {:?} has the wrong kind at {}",
                    requirement.id,
                    canonical.display()
                );
            }
            resolved.insert(requirement.id.clone(), canonical);
        }
        Ok(resolved)
    }
}

#[derive(Clone, Copy)]
enum ResourceSource {
    Override,
    Package,
    PrivateData,
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
            "unsupported RackForge plugin platform {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SERIAL: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn identifies_the_development_platform() {
        let platform = platform_key().unwrap();
        assert!(platform.contains('-'));
    }

    #[test]
    fn resolves_declared_resources_from_private_plugin_data() {
        let base = std::env::temp_dir().join(format!(
            "rackforge-private-resource-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let package_root = base.join("package");
        let data_root = base.join("data");
        fs::create_dir_all(&package_root).unwrap();
        fs::write(
            package_root.join(MANIFEST_FILE),
            r#"
schema_version = 1
id = "org.rackforge.private-resource-test"
name = "Private resource test"
vendor = "RackForge"
version = "0.1.0"
kind = "instrument"
state_version = 1

[api]
major = 1
minor = 5

[[resources]]
id = "program-rom"
name = "Program ROM"
kind = "file"
required = false
data_path = "roms/program.bin"

[binaries]
windows-x86_64 = "plugin.dll"
"#,
        )
        .unwrap();
        let private = PluginStorage::new(&data_root)
            .ensure_plugin("org.rackforge.private-resource-test")
            .unwrap();
        fs::create_dir_all(private.root.join("roms")).unwrap();
        let expected = private.root.join("roms/program.bin");
        fs::write(&expected, b"ROM").unwrap();

        let package = PluginPackage::open(&package_root).unwrap();
        let resolved = package
            .resolve_resources(&BTreeMap::new(), Some(&data_root))
            .unwrap();
        assert_eq!(
            resolved.get("program-rom"),
            Some(&fs::canonicalize(expected).unwrap())
        );
        fs::remove_dir_all(base).unwrap();
    }
}
