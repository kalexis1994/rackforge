use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use rackforge_plugin_api::PluginManifest;
use rackforge_repository::{
    RepositoryConfig, RepositoryFile, fetch_repository, install_archive, install_local_archive,
    install_local_archive_replacing, repository_platform_key, set_plugin_enabled, uninstall_plugin,
    verify_catalog,
};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("RACKFORGE_STORE_ERROR {error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    match arguments.as_slice() {
        [command, secret, public] if command == "keygen" => keygen(secret, public),
        [command, index, secret, signature] if command == "sign" => {
            sign_index(index, secret, signature)
        }
        [command, index, signature, config, repository_id] if command == "verify" => {
            let repository = repository_from_file(config, repository_id)?;
            let index = fs::read(index).map_err(|error| error.to_string())?;
            let signature = fs::read(signature).map_err(|error| error.to_string())?;
            let verified = verify_catalog(&repository, &index, &signature)
                .map_err(|error| error.to_string())?;
            println!(
                "REPOSITORY_VERIFIED id={} plugins={}",
                verified.repository_id,
                verified.plugins.len()
            );
            Ok(())
        }
        [command, config] if command == "list" => list(config),
        [command, config, repository_id, plugin_id, store_root] if command == "install" => {
            install(config, repository_id, plugin_id, store_root, None)
        }
        [
            command,
            config,
            repository_id,
            plugin_id,
            store_root,
            version,
        ] if command == "install" => {
            install(config, repository_id, plugin_id, store_root, Some(version))
        }
        [command, package, store_root, flag]
            if command == "install-local" && flag == "--replace" =>
        {
            // Only a platform installer laying down the release's own
            // pinned, hash-checked package may overwrite a version.
            install_local(package, store_root, true)
        }
        [command, package, store_root] if command == "install-local" => {
            install_local(package, store_root, false)
        }
        [command, plugin_id, store_root] if command == "enable" => {
            set_enabled(plugin_id, store_root, true)
        }
        [command, plugin_id, store_root] if command == "disable" => {
            set_enabled(plugin_id, store_root, false)
        }
        [command, plugin_id, store_root] if command == "uninstall" => {
            uninstall(plugin_id, store_root)
        }
        [command, package, output] if command == "pack" => pack(package, output),
        [command, package, component, output] if command == "pack-wasm" => {
            pack_wasm(package, component, output)
        }
        _ => Err(usage().into()),
    }
}

fn set_enabled(plugin_id: &str, store_root: &str, enabled: bool) -> Result<(), String> {
    set_plugin_enabled(store_root, plugin_id, enabled).map_err(|error| error.to_string())?;
    println!("PLUGIN_ACTIVATION id={plugin_id} enabled={enabled}");
    Ok(())
}

fn uninstall(plugin_id: &str, store_root: &str) -> Result<(), String> {
    let removed = uninstall_plugin(store_root, plugin_id).map_err(|error| error.to_string())?;
    println!(
        "PLUGIN_UNINSTALLED id={} versions={} cleanup_pending={} user_data_preserved=true",
        removed.plugin_id, removed.removed_versions, removed.cleanup_pending
    );
    Ok(())
}

fn install_local(package_path: &str, store_root: &str, replace: bool) -> Result<(), String> {
    let bytes = fs::read(package_path).map_err(|error| error.to_string())?;
    let installed = if replace {
        install_local_archive_replacing(store_root, &bytes)
    } else {
        install_local_archive(store_root, &bytes)
    }
    .map_err(|error| error.to_string())?;
    println!(
        "PLUGIN_INSTALLED id={} version={} path={} existing={} source=local",
        installed.record.plugin_id,
        installed.record.version,
        installed.path.display(),
        installed.already_installed
    );
    Ok(())
}

fn keygen(secret_path: &str, public_path: &str) -> Result<(), String> {
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret).map_err(|error| error.to_string())?;
    let key = SigningKey::from_bytes(&secret);
    write_new(
        secret_path,
        format!("{}\n", STANDARD.encode(key.to_bytes())).as_bytes(),
    )?;
    write_new(
        public_path,
        format!("{}\n", STANDARD.encode(key.verifying_key().to_bytes())).as_bytes(),
    )?;
    println!("REPOSITORY_KEY_CREATED public={public_path}");
    Ok(())
}

fn sign_index(index_path: &str, secret_path: &str, output_path: &str) -> Result<(), String> {
    let bytes = fs::read(index_path).map_err(|error| error.to_string())?;
    let secret = read_base64_exact::<32>(secret_path)?;
    let signature = SigningKey::from_bytes(&secret).sign(&bytes);
    write_new(
        output_path,
        format!("{}\n", STANDARD.encode(signature.to_bytes())).as_bytes(),
    )?;
    println!("REPOSITORY_INDEX_SIGNED path={output_path}");
    Ok(())
}

fn list(config_path: &str) -> Result<(), String> {
    let config = load_config(config_path)?;
    for repository in config.repositories.iter().filter(|item| item.enabled) {
        let verified = fetch_repository(repository).map_err(|error| error.to_string())?;
        println!(
            "REPOSITORY id={} name={:?} plugins={}",
            verified.index.repository_id,
            verified.index.name,
            verified.index.plugins.len()
        );
        for plugin in &verified.index.plugins {
            let latest = plugin
                .releases
                .iter()
                .map(|release| release.version.as_str())
                .max()
                .unwrap_or("none");
            println!(
                "PLUGIN id={} name={:?} latest={} summary={:?}",
                plugin.id, plugin.name, latest, plugin.summary
            );
        }
    }
    Ok(())
}

fn install(
    config_path: &str,
    repository_id: &str,
    plugin_id: &str,
    store_root: &str,
    version: Option<&String>,
) -> Result<(), String> {
    let repository = repository_from_file(config_path, repository_id)?;
    if !repository.enabled {
        return Err(format!("repository {repository_id:?} is disabled"));
    }
    let verified = fetch_repository(&repository).map_err(|error| error.to_string())?;
    let platform = repository_platform_key().map_err(|error| error.to_string())?;
    let selected = verified
        .select(plugin_id, version.map(String::as_str), platform)
        .map_err(|error| error.to_string())?;
    println!(
        "PLUGIN_DOWNLOAD id={} version={} platform={} bytes={}",
        plugin_id, selected.release.version, selected.artifact.platform, selected.artifact.size
    );
    let bytes = verified
        .download(&selected)
        .map_err(|error| error.to_string())?;
    let installed =
        install_archive(store_root, &selected, &bytes).map_err(|error| error.to_string())?;
    println!(
        "PLUGIN_INSTALLED id={} version={} path={} existing={}",
        installed.record.plugin_id,
        installed.record.version,
        installed.path.display(),
        installed.already_installed
    );
    Ok(())
}

fn pack(package_path: &str, output_path: &str) -> Result<(), String> {
    let package = fs::canonicalize(package_path).map_err(|error| error.to_string())?;
    if !package.is_dir() || !package.join("rackforge-plugin.toml").is_file() {
        return Err("package must be a directory containing rackforge-plugin.toml".into());
    }
    if !output_path.ends_with(".rfplugin") {
        return Err("package output must end in .rfplugin".into());
    }
    let mut files = Vec::new();
    collect_files(&package, &package, &mut files)?;
    files.sort();
    let output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output_path)
        .map_err(|error| error.to_string())?;
    let mut archive = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for relative in files {
        let name = relative.to_string_lossy().replace('\\', "/");
        archive
            .start_file(name, options)
            .map_err(|error| error.to_string())?;
        let mut source = File::open(package.join(&relative)).map_err(|error| error.to_string())?;
        std::io::copy(&mut source, &mut archive).map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    let bytes = fs::read(output_path).map_err(|error| error.to_string())?;
    println!(
        "RFPLUGIN_PACKED path={} bytes={} sha256={}",
        output_path,
        bytes.len(),
        hex_digest(&Sha256::digest(&bytes))
    );
    Ok(())
}

fn pack_wasm(package_path: &str, component_path: &str, output_path: &str) -> Result<(), String> {
    let package = fs::canonicalize(package_path).map_err(|error| error.to_string())?;
    let manifest_path = package.join("rackforge-plugin.toml");
    if !package.is_dir() || !manifest_path.is_file() {
        return Err("package must be a directory containing rackforge-plugin.toml".into());
    }
    if !output_path.ends_with(".rfplugin") {
        return Err("package output must end in .rfplugin".into());
    }
    let manifest_text = fs::read_to_string(&manifest_path).map_err(|error| error.to_string())?;
    let manifest: PluginManifest =
        toml::from_str(&manifest_text).map_err(|error| format!("invalid manifest: {error}"))?;
    manifest
        .validate()
        .map_err(|error| format!("invalid manifest: {error}"))?;
    let declared = manifest
        .portable_component()
        .ok_or("manifest does not declare a portable component")?;
    let component = fs::canonicalize(component_path).map_err(|error| error.to_string())?;
    if !component.is_file() {
        return Err("portable component must be a file".into());
    }
    let component_bytes = fs::read(&component).map_err(|error| error.to_string())?;
    if !component_bytes.starts_with(b"\0asm") {
        return Err("portable component is not a WebAssembly binary".into());
    }

    let declared_path = PathBuf::from(&declared.path);
    let mut files = Vec::new();
    collect_files(&package, &package, &mut files)?;
    files.retain(|relative| relative != &declared_path);
    files.sort();
    let output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output_path)
        .map_err(|error| error.to_string())?;
    let mut archive = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for relative in files {
        let name = relative.to_string_lossy().replace('\\', "/");
        archive
            .start_file(name, options)
            .map_err(|error| error.to_string())?;
        let mut source = File::open(package.join(&relative)).map_err(|error| error.to_string())?;
        std::io::copy(&mut source, &mut archive).map_err(|error| error.to_string())?;
    }
    archive
        .start_file(declared.path.replace('\\', "/"), options)
        .map_err(|error| error.to_string())?;
    archive
        .write_all(&component_bytes)
        .map_err(|error| error.to_string())?;
    archive.finish().map_err(|error| error.to_string())?;
    let bytes = fs::read(output_path).map_err(|error| error.to_string())?;
    println!(
        "RFPLUGIN_WASM_PACKED path={} component={} bytes={} sha256={}",
        output_path,
        component.display(),
        bytes.len(),
        hex_digest(&Sha256::digest(&bytes))
    );
    Ok(())
}

fn collect_files(root: &Path, current: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err(format!(
                "symbolic links are forbidden: {}",
                entry.path().display()
            ));
        }
        if file_type.is_dir() {
            collect_files(root, &entry.path(), output)?;
        } else if file_type.is_file() {
            output.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_path_buf(),
            );
        } else {
            return Err(format!(
                "unsupported package entry: {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn load_config(path: &str) -> Result<RepositoryFile, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    RepositoryFile::parse_toml(&bytes).map_err(|error| error.to_string())
}

fn repository_from_file(path: &str, id: &str) -> Result<RepositoryConfig, String> {
    load_config(path)?
        .repositories
        .into_iter()
        .find(|repository| repository.id == id)
        .ok_or_else(|| format!("repository {id:?} is not configured"))
}

fn read_base64_exact<const N: usize>(path: &str) -> Result<[u8; N], String> {
    let mut value = String::new();
    File::open(path)
        .and_then(|mut file| file.read_to_string(&mut value))
        .map_err(|error| error.to_string())?;
    STANDARD
        .decode(value.trim())
        .map_err(|error| error.to_string())?
        .try_into()
        .map_err(|_| format!("{path} must decode to exactly {N} bytes"))
}

fn write_new(path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn usage() -> &'static str {
    "usage:\n  rackforge-store keygen SECRET_KEY PUBLIC_KEY\n  rackforge-store sign INDEX_JSON SECRET_KEY INDEX_SIG\n  rackforge-store verify INDEX_JSON INDEX_SIG REPOSITORIES_TOML REPOSITORY_ID\n  rackforge-store list REPOSITORIES_TOML\n  rackforge-store install REPOSITORIES_TOML REPOSITORY_ID PLUGIN_ID STORE_ROOT [VERSION]\n  rackforge-store install-local PACKAGE.rfplugin STORE_ROOT [--replace]\n  rackforge-store enable PLUGIN_ID STORE_ROOT\n  rackforge-store disable PLUGIN_ID STORE_ROOT\n  rackforge-store uninstall PLUGIN_ID STORE_ROOT\n  rackforge-store pack PACKAGE_DIRECTORY OUTPUT.rfplugin\n  rackforge-store pack-wasm PACKAGE_DIRECTORY COMPONENT_WASM OUTPUT.rfplugin"
}
