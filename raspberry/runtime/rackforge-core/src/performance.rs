use anyhow::{Context, Result, bail};
use rackforge_performance_api::{
    LibraryRevision, LiveLocation, LivePerformanceState, MidiOutputRoute,
    PERFORMANCE_SCHEMA_VERSION, PerformanceEdit, PerformanceLibrary, RackDefinition, RackId,
    RackSlot, RackSlotId, SetlistDefinition, SetlistEntry, SetlistEntryId, SetlistId,
    SongDefinition, SongId, SongPart, SongPartId,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const PERFORMANCE_DIRECTORY: &str = "performance";
const MAX_DOCUMENT_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub struct PerformanceBootstrap {
    pub plugin_id: String,
    pub plugin_state_id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct PerformanceRepository {
    root: Option<PathBuf>,
    library: PerformanceLibrary,
}

impl PerformanceRepository {
    pub fn in_memory(library: PerformanceLibrary) -> Result<Self> {
        library.validate()?;
        Ok(Self {
            root: None,
            library,
        })
    }

    pub fn load_or_bootstrap(
        data_root: Option<&Path>,
        bootstrap: PerformanceBootstrap,
    ) -> Result<Self> {
        let root = data_root.map(|root| root.join(PERFORMANCE_DIRECTORY));
        let mut repository = Self {
            root,
            library: PerformanceLibrary::empty(),
        };
        repository.load()?;
        if repository.library.racks.is_empty()
            && repository.library.songs.is_empty()
            && repository.library.setlists.is_empty()
        {
            repository.library = bootstrap_library(bootstrap)?;
            repository.persist_bootstrap()?;
            println!("PERFORMANCE_LIBRARY_BOOTSTRAPPED");
        }
        repository
            .library
            .validate()
            .context("validating performance library")?;
        Ok(repository)
    }

    pub fn library(&self) -> &PerformanceLibrary {
        &self.library
    }

    pub fn revision(&self) -> LibraryRevision {
        library_revision(&self.library)
    }

    pub fn apply_edit(
        &mut self,
        expected_revision: &LibraryRevision,
        edit: PerformanceEdit,
        live: &LivePerformanceState,
    ) -> Result<()> {
        let current_revision = self.revision();
        if &current_revision != expected_revision {
            bail!(
                "performance library conflict: expected {}, current {}",
                expected_revision.as_str(),
                current_revision.as_str()
            );
        }
        let mut candidate = self.library.clone();
        edit.apply_to(&mut candidate)?;
        live.validate(&candidate)?;
        self.persist_edit(&edit)?;
        self.library = candidate;
        Ok(())
    }

    pub fn initial_live_state(&self) -> LivePerformanceState {
        let Some(rack) = self.library.racks.iter().find(|rack| rack.enabled) else {
            return LivePerformanceState::default();
        };
        let location = LiveLocation::Rack {
            rack_id: rack.id.clone(),
        };
        let mut state = LivePerformanceState::default();
        state.activate(location, rack.id.clone());
        state
    }

    fn load(&mut self) -> Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        self.library.racks = load_documents(&root.join("racks"))?;
        self.library.songs = load_documents(&root.join("songs"))?;
        self.library.setlists = load_documents(&root.join("setlists"))?;
        Ok(())
    }

    fn persist_bootstrap(&self) -> Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        for rack in &self.library.racks {
            write_new_document(&root.join("racks"), rack.id.as_str(), rack)?;
        }
        for song in &self.library.songs {
            write_new_document(&root.join("songs"), song.id.as_str(), song)?;
        }
        for setlist in &self.library.setlists {
            write_new_document(&root.join("setlists"), setlist.id.as_str(), setlist)?;
        }
        Ok(())
    }

    fn persist_edit(&self, edit: &PerformanceEdit) -> Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        match edit {
            PerformanceEdit::PutRack { rack } => {
                write_document(&root.join("racks"), rack.id.as_str(), rack)
            }
            PerformanceEdit::DeleteRack { rack_id } => {
                delete_document(&root.join("racks"), rack_id.as_str())
            }
            PerformanceEdit::PutSong { song } => {
                write_document(&root.join("songs"), song.id.as_str(), song)
            }
            PerformanceEdit::DeleteSong { song_id } => {
                delete_document(&root.join("songs"), song_id.as_str())
            }
            PerformanceEdit::PutSetlist { setlist } => {
                write_document(&root.join("setlists"), setlist.id.as_str(), setlist)
            }
            PerformanceEdit::DeleteSetlist { setlist_id } => {
                delete_document(&root.join("setlists"), setlist_id.as_str())
            }
        }
    }
}

fn library_revision(library: &PerformanceLibrary) -> LibraryRevision {
    let bytes = serde_json::to_vec(library).expect("validated performance library serializes");
    let digest = Sha256::digest(bytes);
    LibraryRevision::new(format!("{digest:x}")).expect("SHA-256 is a valid library revision")
}

fn bootstrap_library(bootstrap: PerformanceBootstrap) -> Result<PerformanceLibrary> {
    let rack_id = RackId::new("rack.imported.current")?;
    let song_id = SongId::new("song.imported.current")?;
    let part_id = SongPartId::new("part.main")?;
    let library = PerformanceLibrary {
        schema_version: PERFORMANCE_SCHEMA_VERSION,
        racks: vec![RackDefinition {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            id: rack_id.clone(),
            name: bootstrap.name.clone(),
            enabled: true,
            slots: vec![RackSlot {
                id: RackSlotId::new("instrument.main")?,
                name: "Main Instrument".into(),
                plugin_id: bootstrap.plugin_id,
                plugin_state_id: Some(bootstrap.plugin_state_id),
                enabled: true,
                midi_input_channel: None,
                midi_output: MidiOutputRoute::None,
                audio_output_bus: "main".into(),
                level_per_mille: 1_000,
                pan_per_mille: 0,
            }],
        }],
        songs: vec![SongDefinition {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            id: song_id.clone(),
            name: bootstrap.name,
            enabled: true,
            parts: vec![SongPart {
                id: part_id.clone(),
                name: "Main".into(),
                rack_id,
            }],
        }],
        setlists: vec![SetlistDefinition {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            id: SetlistId::new("setlist.default")?,
            name: "Default".into(),
            enabled: true,
            entries: vec![SetlistEntry {
                id: SetlistEntryId::new("entry.current")?,
                song_id,
            }],
        }],
    };
    library.validate()?;
    Ok(library)
}

fn load_documents<T: DeserializeOwned>(directory: &Path) -> Result<Vec<T>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", directory.display()));
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("enumerating {}", directory.display()))?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || !file_type.is_file() {
            bail!(
                "performance library contains a non-regular file {}",
                entry.path().display()
            );
        }
        if entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
        {
            bail!(
                "performance library contains an unsupported file {}",
                entry.path().display()
            );
        }
        paths.push(entry.path());
    }
    paths.sort();
    paths.into_iter().map(|path| read_document(&path)).collect()
}

fn read_document<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let length = file.metadata()?.len();
    if length == 0 || length > MAX_DOCUMENT_BYTES {
        bail!(
            "performance document {} has invalid size {length}",
            path.display()
        );
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

fn write_new_document<T: Serialize>(directory: &Path, id: &str, document: &T) -> Result<()> {
    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;
    let destination = directory.join(format!("{id}.json"));
    if destination.exists() {
        bail!("refusing to overwrite {}", destination.display());
    }
    let bytes = serde_json::to_vec_pretty(document)?;
    let temporary = directory.join(format!(".{id}.json.tmp.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .with_context(|| format!("creating {}", temporary.display()))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, &destination)
        .with_context(|| format!("publishing {}", destination.display()))?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

fn write_document<T: Serialize>(directory: &Path, id: &str, document: &T) -> Result<()> {
    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;
    let destination = directory.join(format!("{id}.json"));
    let bytes = serde_json::to_vec_pretty(document)?;
    let temporary = directory.join(format!(".{id}.json.tmp.{}", std::process::id()));
    if temporary.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("removing stale {}", temporary.display()))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .with_context(|| format!("creating {}", temporary.display()))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, &destination)
        .with_context(|| format!("publishing {}", destination.display()))?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

fn delete_document(directory: &Path, id: &str) -> Result<()> {
    let destination = directory.join(format!("{id}.json"));
    fs::remove_file(&destination).with_context(|| format!("deleting {}", destination.display()))?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rackforge-performance-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn bootstrap() -> PerformanceBootstrap {
        PerformanceBootstrap {
            plugin_id: "org.rackforge.rf-dls".into(),
            plugin_state_id: "dls.b00000000.p00000002".into(),
            name: "Piano 3".into(),
        }
    }

    #[test]
    fn bootstraps_once_then_loads_the_same_valid_graph() {
        let root = temporary_root("bootstrap");
        let first = PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        let second = PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        assert_eq!(first.library(), second.library());
        assert_eq!(first.library().racks[0].name, "Piano 3");
        assert!(
            root.join("performance/racks/rack.imported.current.json")
                .is_file()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_dangling_documents_instead_of_partially_loading() {
        let root = temporary_root("dangling");
        let repository =
            PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        let song_path = root.join("performance/songs/song.imported.current.json");
        let mut song = repository.library().songs[0].clone();
        song.parts[0].rack_id = RackId::new("rack.missing").unwrap();
        fs::write(&song_path, serde_json::to_vec(&song).unwrap()).unwrap();
        assert!(PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn edits_are_atomic_revision_checked_and_survive_reload() {
        let root = temporary_root("edit");
        let mut repository =
            PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        let revision = repository.revision();
        let live = repository.initial_live_state();
        let mut rack = repository.library().racks[0].clone();
        rack.name = "Stage Piano".into();
        repository
            .apply_edit(
                &revision,
                PerformanceEdit::PutRack { rack: rack.clone() },
                &live,
            )
            .unwrap();
        assert_ne!(repository.revision(), revision);
        assert!(
            repository
                .apply_edit(&revision, PerformanceEdit::PutRack { rack }, &live)
                .unwrap_err()
                .to_string()
                .contains("conflict")
        );
        let reloaded = PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        assert_eq!(reloaded.library().racks[0].name, "Stage Piano");
        assert_eq!(reloaded.revision(), repository.revision());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_a_referenced_or_active_rack_is_rejected_before_disk_changes() {
        let root = temporary_root("protected-delete");
        let mut repository =
            PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        let revision = repository.revision();
        let live = repository.initial_live_state();
        let rack_id = repository.library().racks[0].id.clone();
        assert!(
            repository
                .apply_edit(&revision, PerformanceEdit::DeleteRack { rack_id }, &live,)
                .is_err()
        );
        assert_eq!(repository.revision(), revision);
        assert_eq!(repository.library().racks.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
