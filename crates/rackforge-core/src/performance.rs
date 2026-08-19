use anyhow::{Context, Result, bail};
use rackforge_performance_api::{
    LibraryRevision, LiveLocation, LivePerformanceState, MidiOutputRoute,
    PERFORMANCE_SCHEMA_VERSION, PerformanceEdit, PerformanceLibrary, RackDefinition, RackId,
    RackSlot, RackSlotId, SetlistDefinition, SetlistEntry, SetlistEntryId, SetlistId,
    SongDefinition, SongId, SongPart, SongPartId,
};
use rackforge_plugin_api::PluginStateReference;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const PERFORMANCE_DIRECTORY: &str = "performance";
const INITIALIZED_MARKER: &str = ".initialized";
const MAX_DOCUMENT_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub struct PerformanceBootstrap {
    pub plugin_id: String,
    pub state: PluginStateReference,
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
        let initialized = repository
            .root
            .as_ref()
            .is_some_and(|root| root.join(INITIALIZED_MARKER).is_file());
        repository.load()?;
        if repository.library.racks.is_empty()
            && repository.library.songs.is_empty()
            && repository.library.setlists.is_empty()
            && !initialized
        {
            repository.library = bootstrap_library(bootstrap)?;
            repository.persist_bootstrap()?;
            println!("PERFORMANCE_LIBRARY_BOOTSTRAPPED");
        }
        repository.persist_initialized_marker()?;
        repository
            .library
            .validate()
            .context("validating performance library")?;
        Ok(repository)
    }

    /// Loads an existing library without inventing a playable Rack. Every
    /// RackForge host boots through this path: a first start has an empty
    /// library and the performer builds the first Rack deliberately. The
    /// bootstrap variant above remains for the browser demo, which must
    /// sound the moment the page opens.
    pub fn load_or_empty(data_root: Option<&Path>) -> Result<Self> {
        let root = data_root.map(|root| root.join(PERFORMANCE_DIRECTORY));
        let mut repository = Self {
            root,
            library: PerformanceLibrary::empty(),
        };
        repository.load()?;
        repository.persist_initialized_marker()?;
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
        live: &mut LivePerformanceState,
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
        let mut candidate_live = live.clone();
        candidate_live.reconcile(&candidate);
        candidate_live.validate(&candidate)?;
        self.persist_edit(&edit)?;
        self.library = candidate;
        *live = candidate_live;
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

    pub fn migrate_legacy_plugin_states(
        &mut self,
        plugin_id: &str,
        mut migrate: impl FnMut(&str) -> Result<PluginStateReference>,
    ) -> Result<usize> {
        let mut migrated = 0;
        let mut changed_racks = Vec::new();
        for rack in &mut self.library.racks {
            let mut changed = false;
            for slot in &mut rack.slots {
                if slot.plugin_id != plugin_id || slot.state.is_some() {
                    continue;
                }
                let Some(program_id) = slot.legacy_program_id.as_deref() else {
                    continue;
                };
                slot.state = Some(migrate(program_id).with_context(|| {
                    format!("migrating legacy state for Rack Slot {}", slot.id)
                })?);
                slot.legacy_program_id = None;
                changed = true;
                migrated += 1;
            }
            if changed {
                rack.validate()?;
                changed_racks.push(rack.clone());
            }
        }
        if let Some(root) = &self.root {
            for rack in changed_racks {
                write_document(&root.join("racks"), rack.id.as_str(), &rack)?;
            }
        }
        Ok(migrated)
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

    fn persist_initialized_marker(&self) -> Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
        let marker = root.join(INITIALIZED_MARKER);
        if marker.is_file() {
            return Ok(());
        }
        let temporary = root.join(format!(".initialized.tmp.{}", crate::storage::writer_discriminator()));
        if temporary.exists() {
            fs::remove_file(&temporary)
                .with_context(|| format!("removing stale {}", temporary.display()))?;
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("creating {}", temporary.display()))?;
        file.write_all(b"RackForge performance library initialized\n")?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, &marker)?;
        sync_directory(root)
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
            keyboard_parts: None,
            graph: None,
            slots: vec![RackSlot {
                id: RackSlotId::new("instrument.main")?,
                name: "Main Instrument".into(),
                plugin_id: bootstrap.plugin_id,
                state: Some(bootstrap.state),
                legacy_program_id: None,
                enabled: true,
                midi_input_channel: None,
                midi_note_low: 0,
                midi_note_high: 127,
                midi_transpose: 0,
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
                content: None,
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
    let temporary = directory.join(format!(".{id}.json.tmp.{}", crate::storage::writer_discriminator()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("creating {}", temporary.display()))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    replace_file(&temporary, &destination)?;
    sync_directory(directory)?;
    Ok(())
}

fn write_document<T: Serialize>(directory: &Path, id: &str, document: &T) -> Result<()> {
    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;
    let destination = directory.join(format!("{id}.json"));
    let bytes = serde_json::to_vec_pretty(document)?;
    let temporary = directory.join(format!(".{id}.json.tmp.{}", crate::storage::writer_discriminator()));
    if temporary.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("removing stale {}", temporary.display()))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("creating {}", temporary.display()))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    replace_file(&temporary, &destination)?;
    sync_directory(directory)?;
    Ok(())
}

fn delete_document(directory: &Path, id: &str) -> Result<()> {
    let destination = directory.join(format!("{id}.json"));
    fs::remove_file(&destination).with_context(|| format!("deleting {}", destination.display()))?;
    sync_directory(directory)?;
    Ok(())
}

#[cfg(unix)]
fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    fs::rename(temporary, destination)
        .with_context(|| format!("publishing {}", destination.display()))
}

#[cfg(not(unix))]
fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    if !destination.exists() {
        return fs::rename(temporary, destination)
            .with_context(|| format!("publishing {}", destination.display()));
    }
    let backup = destination.with_extension("previous-rackforge-performance");
    if backup.exists() {
        fs::remove_file(&backup).with_context(|| format!("removing {}", backup.display()))?;
    }
    fs::rename(destination, &backup)
        .with_context(|| format!("backing up {}", destination.display()))?;
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::rename(&backup, destination);
        return Err(error).with_context(|| format!("publishing {}", destination.display()));
    }
    fs::remove_file(&backup).with_context(|| format!("removing {}", backup.display()))
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<()> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .with_context(|| format!("syncing directory {}", directory.display()))
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<()> {
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
            crate::storage::writer_discriminator()
        ))
    }

    fn bootstrap() -> PerformanceBootstrap {
        PerformanceBootstrap {
            plugin_id: "org.rackforge.rf-dls".into(),
            state: PluginStateReference {
                schema_version: rackforge_plugin_api::PLUGIN_STATE_REFERENCE_SCHEMA_VERSION,
                plugin_id: "org.rackforge.rf-dls".into(),
                plugin_version: "0.1.0".into(),
                state_version: 3,
                blob_sha256: "a".repeat(64),
                byte_length: 16,
                selected_sound_id: Some("dls.b00000000.p00000002".into()),
            },
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
    fn an_intentionally_emptied_library_is_not_bootstrapped_again() {
        let root = temporary_root("persistent-empty");
        let mut repository =
            PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        let mut live = repository.initial_live_state();
        let setlist_id = repository.library().setlists[0].id.clone();
        let song_id = repository.library().songs[0].id.clone();
        let rack_id = repository.library().racks[0].id.clone();

        for edit in [
            PerformanceEdit::DeleteSetlist { setlist_id },
            PerformanceEdit::DeleteSong { song_id },
            PerformanceEdit::DeleteRack { rack_id },
        ] {
            let revision = repository.revision();
            repository.apply_edit(&revision, edit, &mut live).unwrap();
        }
        assert!(root.join("performance/.initialized").is_file());
        drop(repository);

        let reloaded = PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        assert!(reloaded.library().racks.is_empty());
        assert!(reloaded.library().songs.is_empty());
        assert!(reloaded.library().setlists.is_empty());
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
        let mut live = repository.initial_live_state();
        let mut rack = repository.library().racks[0].clone();
        rack.name = "Stage Piano".into();
        repository
            .apply_edit(
                &revision,
                PerformanceEdit::PutRack { rack: rack.clone() },
                &mut live,
            )
            .unwrap();
        assert_ne!(repository.revision(), revision);
        assert!(
            repository
                .apply_edit(&revision, PerformanceEdit::PutRack { rack }, &mut live)
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
        let mut live = repository.initial_live_state();
        let rack_id = repository.library().racks[0].id.clone();
        assert!(
            repository
                .apply_edit(
                    &revision,
                    PerformanceEdit::DeleteRack { rack_id },
                    &mut live,
                )
                .is_err()
        );
        assert_eq!(repository.revision(), revision);
        assert_eq!(repository.library().racks.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_the_active_setlist_preserves_its_sounding_rack() {
        let root = temporary_root("delete-active-setlist");
        let mut repository =
            PerformanceRepository::load_or_bootstrap(Some(&root), bootstrap()).unwrap();
        let revision = repository.revision();
        let rack_id = repository.library().racks[0].id.clone();
        let setlist_id = repository.library().setlists[0].id.clone();
        let location = LiveLocation::Setlist {
            setlist_id: setlist_id.clone(),
            entry_id: repository.library().setlists[0].entries[0].id.clone(),
            part_id: repository.library().songs[0].parts[0].id.clone(),
        };
        let mut live = LivePerformanceState::default();
        live.activate(location, rack_id.clone());

        repository
            .apply_edit(
                &revision,
                PerformanceEdit::DeleteSetlist { setlist_id },
                &mut live,
            )
            .unwrap();

        assert!(repository.library().setlists.is_empty());
        assert_eq!(
            live.active,
            Some(LiveLocation::Rack {
                rack_id: rack_id.clone(),
            })
        );
        assert_eq!(live.active_rack_id, Some(rack_id));
        live.validate(repository.library()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
