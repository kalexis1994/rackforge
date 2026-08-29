//! The `.rflive` show document: assemble, inspect, apply.
//!
//! A show is the whole performance library plus every plugin state its
//! Racks reference, embedded so the file is self-contained. The logic
//! lives here once; each host wires it to its own library lock and state
//! store. Importing never deletes: every document upserts through the
//! library's own edit machinery, so an id collision replaces that entry
//! and everything else on the machine is kept.

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rackforge_control_api::{
    RFLIVE_FORMAT, RFLIVE_SCHEMA_VERSION, RfLiveEmbeddedState, RfLiveFile, RfLiveImportPreview,
    RfLiveRequirement,
};
use rackforge_performance_api::{PerformanceEdit, PerformanceLibrary};
use std::collections::{BTreeMap, BTreeSet};

use crate::state_store::{MAX_PLUGIN_STATE_BYTES, PluginStateStore};

/// Assembles the show: the library as it stands, plus every state a Rack
/// Slot references, read from the store and embedded. States deduplicate
/// by blob hash; requirements deduplicate by plugin id, keeping the
/// version the embedded states were made with.
pub fn assemble_live_show(
    name: &str,
    library: &PerformanceLibrary,
    store: &PluginStateStore,
    exported_unix_ms: u64,
) -> Result<RfLiveFile> {
    let name = name.trim();
    if name.is_empty() {
        bail!("the show needs a name");
    }
    let mut states = Vec::new();
    let mut seen_blobs = BTreeSet::new();
    let mut requirements: BTreeMap<String, String> = BTreeMap::new();
    for rack in &library.racks {
        for slot in &rack.slots {
            let version = slot
                .state
                .as_ref()
                .map(|state| state.plugin_version.clone());
            match requirements.entry(slot.plugin_id.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(version.unwrap_or_default());
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if entry.get().is_empty()
                        && let Some(version) = version
                    {
                        entry.insert(version);
                    }
                }
            }
            let Some(reference) = &slot.state else {
                continue;
            };
            if !seen_blobs.insert(reference.blob_sha256.clone()) {
                continue;
            }
            let bytes = store.read(reference).with_context(|| {
                format!(
                    "reading the state Rack {:?} keeps for {}",
                    rack.name, slot.plugin_id
                )
            })?;
            states.push(RfLiveEmbeddedState {
                reference: reference.clone(),
                state_base64: BASE64.encode(bytes),
            });
        }
    }
    Ok(RfLiveFile {
        format: RFLIVE_FORMAT.into(),
        schema_version: RFLIVE_SCHEMA_VERSION,
        exported_by: format!("RackForge {}", env!("CARGO_PKG_VERSION")),
        exported_unix_ms,
        name: name.into(),
        library: library.clone(),
        states,
        requirements: requirements
            .into_iter()
            .map(|(plugin_id, version)| RfLiveRequirement { plugin_id, version })
            .collect(),
    })
}

/// The moment of export, for the file's own record.
pub fn now_unix_ms() -> Result<u64> {
    Ok(u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?)
}

/// The portable file name for a show: `<name>.rflive`, characters a
/// pendrive between machines will accept.
pub fn live_show_file_name(name: &str) -> String {
    let mut output = String::new();
    for character in name.trim().chars().take(80) {
        if character.is_alphanumeric() || matches!(character, '-' | '.' | '_') {
            output.push(character);
        } else if character.is_whitespace() {
            if !output.is_empty() && !output.ends_with(' ') {
                output.push(' ');
            }
        } else if !output.is_empty() && !output.ends_with('_') {
            output.push('_');
        }
    }
    let output = output.trim_matches(['.', '_', ' ']);
    if output.is_empty() {
        "RackForge Show.rflive".into()
    } else {
        format!("{output}.rflive")
    }
}

/// Validates a show against this machine without changing anything.
/// Authenticity is non-negotiable: every embedded state must decode to
/// its declared length and reference. Everything else — missing plugins,
/// version drift, entries the upsert would replace — is a warning the
/// musician decides on.
pub fn inspect_live_show(
    file: &RfLiveFile,
    installed: &BTreeMap<String, String>,
    current: &PerformanceLibrary,
) -> Result<RfLiveImportPreview> {
    if file.format != RFLIVE_FORMAT {
        bail!("not a RackForge show file (format {:?})", file.format);
    }
    if file.schema_version != RFLIVE_SCHEMA_VERSION {
        bail!("unsupported .rflive schema {}", file.schema_version);
    }
    let mut warnings = Vec::new();
    for state in &file.states {
        state
            .reference
            .validate()
            .context("validating an embedded plugin state reference")?;
        let bytes = decode_state(state)?;
        if bytes.len() != state.reference.byte_length as usize {
            bail!(
                "embedded state for {} does not match its declared length",
                state.reference.plugin_id
            );
        }
    }
    let mut missing_plugins = Vec::new();
    for requirement in &file.requirements {
        match installed.get(&requirement.plugin_id) {
            None => missing_plugins.push(requirement.clone()),
            Some(version) => {
                if !requirement.version.is_empty() && version != &requirement.version {
                    warnings.push(format!(
                        "{} is installed as v{version}; the show was made with v{}.",
                        requirement.plugin_id, requirement.version
                    ));
                }
            }
        }
    }
    push_collision_warning(
        &mut warnings,
        "Rack",
        current.racks.iter().map(|item| &item.id),
        file.library
            .racks
            .iter()
            .map(|item| (&item.id, item.name.as_str())),
    );
    push_collision_warning(
        &mut warnings,
        "Song",
        current.songs.iter().map(|item| &item.id),
        file.library
            .songs
            .iter()
            .map(|item| (&item.id, item.name.as_str())),
    );
    push_collision_warning(
        &mut warnings,
        "Setlist",
        current.setlists.iter().map(|item| &item.id),
        file.library
            .setlists
            .iter()
            .map(|item| (&item.id, item.name.as_str())),
    );
    push_collision_warning(
        &mut warnings,
        "Pattern",
        current.patterns.iter().map(|item| &item.id),
        file.library
            .patterns
            .iter()
            .map(|item| (&item.id, item.name.as_str())),
    );
    Ok(RfLiveImportPreview {
        name: file.name.clone(),
        racks: file.library.racks.len() as u32,
        songs: file.library.songs.len() as u32,
        setlists: file.library.setlists.len() as u32,
        patterns: file.library.patterns.len() as u32,
        states: file.states.len() as u32,
        missing_plugins,
        warnings,
    })
}

fn push_collision_warning<'item, Id: PartialEq + 'item>(
    warnings: &mut Vec<String>,
    kind: &str,
    current_ids: impl Iterator<Item = &'item Id>,
    incoming: impl Iterator<Item = (&'item Id, &'item str)>,
) {
    let current: Vec<&Id> = current_ids.collect();
    let replaced: Vec<&str> = incoming
        .filter(|(id, _)| current.contains(id))
        .map(|(_, name)| name)
        .collect();
    match replaced.len() {
        0 => {}
        1 => warnings.push(format!("Replaces the existing {kind} {:?}.", replaced[0])),
        count => warnings.push(format!("Replaces {count} existing {kind}s.")),
    }
}

/// Writes the embedded states into the store, verifying that each blob
/// hashes back to the reference the Racks point at. Content-addressed, so
/// re-importing a show is idempotent.
pub fn store_live_show_states(file: &RfLiveFile, store: &mut PluginStateStore) -> Result<u32> {
    let mut stored = 0u32;
    for state in &file.states {
        let bytes = decode_state(state)?;
        let written = store
            .put(
                &state.reference.plugin_id,
                &state.reference.plugin_version,
                state.reference.state_version,
                state.reference.selected_sound_id.clone(),
                &bytes,
            )
            .with_context(|| format!("storing a state for {}", state.reference.plugin_id))?;
        if written.blob_sha256 != state.reference.blob_sha256 {
            bail!(
                "embedded state for {} does not hash to its reference — the file was altered",
                state.reference.plugin_id
            );
        }
        stored += 1;
    }
    Ok(stored)
}

/// The import as library edits, ready for each host's own revisioned edit
/// path: every document upserts, nothing deletes.
pub fn live_show_edits(file: &RfLiveFile) -> Vec<PerformanceEdit> {
    let library = &file.library;
    let mut edits = Vec::with_capacity(
        library.racks.len() + library.songs.len() + library.setlists.len() + library.patterns.len(),
    );
    for rack in &library.racks {
        edits.push(PerformanceEdit::PutRack { rack: rack.clone() });
    }
    for song in &library.songs {
        edits.push(PerformanceEdit::PutSong { song: song.clone() });
    }
    for setlist in &library.setlists {
        edits.push(PerformanceEdit::PutSetlist {
            setlist: setlist.clone(),
        });
    }
    for pattern in &library.patterns {
        edits.push(PerformanceEdit::PutPattern {
            pattern: pattern.clone(),
        });
    }
    edits
}

fn decode_state(state: &RfLiveEmbeddedState) -> Result<Vec<u8>> {
    let bytes = BASE64
        .decode(state.state_base64.as_bytes())
        .context("decoding an embedded plugin state")?;
    if bytes.is_empty() || bytes.len() > MAX_PLUGIN_STATE_BYTES {
        bail!(
            "embedded plugin state size {} is outside 1..={MAX_PLUGIN_STATE_BYTES}",
            bytes.len()
        );
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_performance_api::{
        MidiOutputRoute, PERFORMANCE_SCHEMA_VERSION, RackDefinition, RackId, RackSlot, RackSlotId,
    };

    fn library_with_one_state(store: &mut PluginStateStore) -> PerformanceLibrary {
        let reference = store
            .put("org.rackforge.rf-dls", "1.2.3", 1, None, b"the sound")
            .expect("state stored");
        PerformanceLibrary {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            racks: vec![RackDefinition {
                schema_version: PERFORMANCE_SCHEMA_VERSION,
                id: RackId::new("rack.show").unwrap(),
                name: "Show Rack".into(),
                enabled: true,
                keyboard_parts: None,
                slots: vec![RackSlot {
                    id: RackSlotId::new("instrument.main").unwrap(),
                    name: "Main Instrument".into(),
                    plugin_id: "org.rackforge.rf-dls".into(),
                    state: Some(reference),
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
                graph: None,
            }],
            songs: Vec::new(),
            setlists: Vec::new(),
            patterns: Vec::new(),
        }
    }

    #[test]
    fn a_show_round_trips_through_a_second_machine() {
        let mut studio = PluginStateStore::new(None).expect("store");
        let library = library_with_one_state(&mut studio);
        let file =
            assemble_live_show("Friday Set", &library, &studio, 1_000).expect("assembled");
        assert_eq!(file.states.len(), 1);
        assert_eq!(file.requirements.len(), 1);
        assert_eq!(file.requirements[0].version, "1.2.3");

        // The venue machine has the plugin but has never seen the state.
        let mut venue = PluginStateStore::new(None).expect("store");
        let installed =
            BTreeMap::from([("org.rackforge.rf-dls".to_owned(), "1.2.3".to_owned())]);
        let preview =
            inspect_live_show(&file, &installed, &PerformanceLibrary::default())
                .expect("inspected");
        assert_eq!(preview.racks, 1);
        assert_eq!(preview.states, 1);
        assert!(preview.missing_plugins.is_empty());
        assert!(preview.warnings.is_empty());

        assert_eq!(store_live_show_states(&file, &mut venue).expect("stored"), 1);
        let edits = live_show_edits(&file);
        assert_eq!(edits.len(), 1);
        // The imported Rack's reference now resolves on the venue machine.
        let slot_state = library.racks[0].slots[0].state.as_ref().unwrap();
        assert_eq!(venue.read(slot_state).expect("blob"), b"the sound");
    }

    #[test]
    fn inspection_names_what_is_missing_and_what_it_replaces() {
        let mut studio = PluginStateStore::new(None).expect("store");
        let library = library_with_one_state(&mut studio);
        let file = assemble_live_show("Set", &library, &studio, 1_000).expect("assembled");
        // No plugins installed, and the same Rack id already exists.
        let preview = inspect_live_show(&file, &BTreeMap::new(), &library).expect("inspected");
        assert_eq!(preview.missing_plugins.len(), 1);
        assert_eq!(preview.missing_plugins[0].plugin_id, "org.rackforge.rf-dls");
        assert!(preview.warnings.iter().any(|w| w.contains("Show Rack")));
    }

    #[test]
    fn a_tampered_state_is_refused_before_anything_is_written() {
        let mut studio = PluginStateStore::new(None).expect("store");
        let library = library_with_one_state(&mut studio);
        let mut file = assemble_live_show("Set", &library, &studio, 1_000).expect("assembled");
        file.states[0].state_base64 = BASE64.encode(b"hostile bytes");
        // Inspection catches the length lie; the store catches the hash lie.
        assert!(inspect_live_show(&file, &BTreeMap::new(), &PerformanceLibrary::default())
            .is_err());
        let mut venue = PluginStateStore::new(None).expect("store");
        assert!(store_live_show_states(&file, &mut venue).is_err());
    }

    #[test]
    fn show_file_names_stay_portable() {
        assert_eq!(live_show_file_name("Friday Set"), "Friday Set.rflive");
        assert_eq!(live_show_file_name("  "), "RackForge Show.rflive");
        assert_eq!(live_show_file_name("Jarre / Lyon: 2026"), "Jarre _ Lyon_ 2026.rflive");
    }
}
