use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const ROOT_SCHEMA_VERSION: u32 = 1;
const BOOTSTRAP_FILE: &str = "bootstrap.toml";
const PORTABLE_FILE: &str = "rackforge-portable.toml";
static WRITE_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RootMode {
    Standard,
    Custom,
    Portable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootChoice {
    pub mode: RootMode,
    pub root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct DesktopPaths {
    pub root: PathBuf,
    pub legacy_plugins_root: PathBuf,
    pub plugin_store_root: PathBuf,
    pub data_root: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RootDocument {
    schema_version: u32,
    mode: RootMode,
    root: PathBuf,
}

impl DesktopPaths {
    pub fn initialize(root: impl AsRef<Path>) -> Result<Self> {
        let root = absolute_path(root.as_ref())?;
        let paths = Self {
            legacy_plugins_root: root.join("plugins"),
            plugin_store_root: root.join("plugin-store"),
            data_root: root.join("data"),
            root,
        };
        for directory in [
            paths.root.clone(),
            paths.legacy_plugins_root.clone(),
            paths.plugin_store_root.join("packages"),
            paths.plugin_store_root.join("records"),
            paths.data_root.join("plugins"),
            paths.root.join("config"),
            paths.root.join("sessions"),
            paths.root.join("state"),
            paths.root.join("cache"),
            paths.root.join("logs"),
        ] {
            fs::create_dir_all(&directory)
                .with_context(|| format!("creating RackForge directory {}", directory.display()))?;
        }
        Ok(paths)
    }
}

pub fn default_root() -> Result<PathBuf> {
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(local).join("RackForge"));
    }
    Ok(env::current_dir()?.join(".rackforge"))
}

pub fn executable_directory() -> Result<PathBuf> {
    env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .context("RackForge executable has no parent directory")
}

pub fn portable_root(executable_directory: &Path) -> PathBuf {
    executable_directory.join("RackForgeData")
}

pub fn load_choice(default_root: &Path, executable_directory: &Path) -> Result<Option<RootChoice>> {
    let portable_path = executable_directory.join(PORTABLE_FILE);
    if portable_path.is_file() {
        let document = read_document(&portable_path)?;
        if document.mode != RootMode::Portable {
            bail!("{} must declare portable mode", portable_path.display());
        }
        return Ok(Some(RootChoice {
            mode: document.mode,
            root: resolve_document_root(&document.root, executable_directory),
        }));
    }

    let bootstrap_path = default_root.join(BOOTSTRAP_FILE);
    if !bootstrap_path.is_file() {
        return Ok(None);
    }
    let document = read_document(&bootstrap_path)?;
    if document.mode == RootMode::Portable {
        bail!(
            "{} cannot select portable mode; use {} next to RackForge",
            bootstrap_path.display(),
            PORTABLE_FILE
        );
    }
    if !document.root.is_absolute() {
        bail!("{} must contain an absolute root", bootstrap_path.display());
    }
    Ok(Some(RootChoice {
        mode: document.mode,
        root: document.root,
    }))
}

pub fn persist_choice(
    choice: &RootChoice,
    default_root: &Path,
    executable_directory: &Path,
) -> Result<DesktopPaths> {
    let paths = DesktopPaths::initialize(&choice.root)?;
    match choice.mode {
        RootMode::Standard | RootMode::Custom => {
            let document = RootDocument {
                schema_version: ROOT_SCHEMA_VERSION,
                mode: choice.mode,
                root: paths.root.clone(),
            };
            write_document(&default_root.join(BOOTSTRAP_FILE), &document)?;
        }
        RootMode::Portable => {
            let relative = paths
                .root
                .strip_prefix(executable_directory)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| paths.root.clone());
            let document = RootDocument {
                schema_version: ROOT_SCHEMA_VERSION,
                mode: RootMode::Portable,
                root: relative,
            };
            write_document(&executable_directory.join(PORTABLE_FILE), &document)?;
        }
    }
    Ok(paths)
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("RackForge root cannot be empty");
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn resolve_document_root(root: &Path, base: &Path) -> PathBuf {
    if root.is_absolute() {
        root.to_path_buf()
    } else {
        base.join(root)
    }
}

fn read_document(path: &Path) -> Result<RootDocument> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let document: RootDocument =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    if document.schema_version != ROOT_SCHEMA_VERSION {
        bail!(
            "unsupported RackForge root schema {} in {}",
            document.schema_version,
            path.display()
        );
    }
    if document.root.as_os_str().is_empty() {
        bail!("{} contains an empty root", path.display());
    }
    Ok(document)
}

fn write_document(path: &Path, document: &RootDocument) -> Result<()> {
    let parent = path
        .parent()
        .context("RackForge root configuration has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let text = toml::to_string_pretty(document)?;
    let serial = WRITE_SERIAL.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{serial}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("creating {}", temporary.display()))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("replacing {}", path.display()))?;
    }
    fs::rename(&temporary, path)
        .with_context(|| format!("activating RackForge root configuration {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(label: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "rackforge-desktop-paths-{label}-{}-{}",
            std::process::id(),
            WRITE_SERIAL.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn custom_root_is_persisted_and_creates_the_complete_layout() {
        let base = test_root("custom");
        let default = base.join("local/RackForge");
        let executable = base.join("app");
        let custom = base.join("library");
        fs::create_dir_all(&executable).unwrap();
        let paths = persist_choice(
            &RootChoice {
                mode: RootMode::Custom,
                root: custom.clone(),
            },
            &default,
            &executable,
        )
        .unwrap();
        assert_eq!(paths.root, custom);
        assert!(paths.plugin_store_root.join("packages").is_dir());
        assert!(paths.plugin_store_root.join("records").is_dir());
        assert!(paths.data_root.join("plugins").is_dir());
        assert!(paths.root.join("config").is_dir());
        assert_eq!(
            load_choice(&default, &executable).unwrap(),
            Some(RootChoice {
                mode: RootMode::Custom,
                root: paths.root,
            })
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn portable_root_is_relative_to_the_executable_directory() {
        let base = test_root("portable");
        let default = base.join("local/RackForge");
        let executable = base.join("portable-app");
        fs::create_dir_all(&executable).unwrap();
        let choice = RootChoice {
            mode: RootMode::Portable,
            root: portable_root(&executable),
        };
        persist_choice(&choice, &default, &executable).unwrap();
        let text = fs::read_to_string(executable.join(PORTABLE_FILE)).unwrap();
        assert!(text.contains("root = \"RackForgeData\""));
        assert_eq!(load_choice(&default, &executable).unwrap(), Some(choice));
        fs::remove_dir_all(base).unwrap();
    }
}
