use anyhow::{Context, Result};
use rackforge_plugin_api::{ParameterKind, ParameterSchema};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const LIVE_PARAMETER_STATE_SCHEMA_VERSION: u32 = 1;
const LIVE_PARAMETER_STATE_FILE: &str = "live-parameters.json";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PluginParameterState {
    plugin_version: String,
    values: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveParameterStateDocument {
    schema_version: u32,
    #[serde(default)]
    plugins: BTreeMap<String, PluginParameterState>,
}

impl Default for LiveParameterStateDocument {
    fn default() -> Self {
        Self {
            schema_version: LIVE_PARAMETER_STATE_SCHEMA_VERSION,
            plugins: BTreeMap::new(),
        }
    }
}

/// Host-owned checkpoint for public, live plugin parameters.
///
/// Presets remain plugin-owned. This document only records the user's hot
/// adjustments made after loading a preset, keyed by stable parameter ID so a
/// compatible plugin update may reorder numeric indexes safely.
pub struct LiveParameterStateStore {
    path: Option<PathBuf>,
    document: LiveParameterStateDocument,
    dirty: bool,
}

impl LiveParameterStateStore {
    pub fn open(data_root: Option<&Path>) -> Result<Self> {
        let path = data_root.map(|root| root.join("states").join(LIVE_PARAMETER_STATE_FILE));
        let document = match path.as_ref().filter(|path| path.exists()) {
            Some(path) => {
                let bytes = fs::read(path)
                    .with_context(|| format!("reading live parameter state {}", path.display()))?;
                match serde_json::from_slice::<LiveParameterStateDocument>(&bytes) {
                    Ok(document)
                        if document.schema_version == LIVE_PARAMETER_STATE_SCHEMA_VERSION =>
                    {
                        document
                    }
                    Ok(document) => {
                        eprintln!(
                            "LIVE_PARAMETER_STATE_IGNORED path={} schema={} expected={}",
                            path.display(),
                            document.schema_version,
                            LIVE_PARAMETER_STATE_SCHEMA_VERSION
                        );
                        LiveParameterStateDocument::default()
                    }
                    Err(error) => {
                        eprintln!(
                            "LIVE_PARAMETER_STATE_IGNORED path={} reason={error}",
                            path.display()
                        );
                        LiveParameterStateDocument::default()
                    }
                }
            }
            None => LiveParameterStateDocument::default(),
        };
        Ok(Self {
            path,
            document,
            dirty: false,
        })
    }

    pub fn restored_values(&self, plugin_id: &str, schema: &ParameterSchema) -> Vec<(u32, f64)> {
        let Some(plugin) = self.document.plugins.get(plugin_id) else {
            return Vec::new();
        };
        schema
            .parameters
            .iter()
            .filter(|parameter| persistent_parameter(parameter))
            .filter_map(|parameter| {
                let value = *plugin.values.get(&parameter.id)?;
                crate::validate_parameter_write(schema, parameter.index, value)
                    .ok()
                    .map(|_| (parameter.index, value))
            })
            .collect()
    }

    pub fn record(
        &mut self,
        plugin_id: &str,
        plugin_version: &str,
        schema: &ParameterSchema,
        parameter_index: u32,
        canonical_value: f64,
    ) -> Result<bool> {
        let parameter = crate::validate_parameter_write(schema, parameter_index, canonical_value)?;
        if !persistent_parameter(parameter) {
            return Ok(false);
        }
        let plugin = self
            .document
            .plugins
            .entry(plugin_id.to_owned())
            .or_default();
        plugin.plugin_version = plugin_version.to_owned();
        let changed =
            plugin.values.insert(parameter.id.clone(), canonical_value) != Some(canonical_value);
        self.dirty |= changed;
        Ok(changed)
    }

    pub fn clear_plugin(&mut self, plugin_id: &str) -> bool {
        let changed = self.document.plugins.remove(plugin_id).is_some();
        self.dirty |= changed;
        changed
    }

    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let Some(path) = &self.path else {
            self.dirty = false;
            return Ok(());
        };
        let parent = path
            .parent()
            .context("live parameter state has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("creating live parameter state dir {}", parent.display()))?;
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&self.document)
            .context("serializing live parameter state")?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .with_context(|| {
                format!(
                    "opening temporary live parameter state {}",
                    temporary.display()
                )
            })?;
        file.write_all(&bytes).with_context(|| {
            format!(
                "writing temporary live parameter state {}",
                temporary.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "syncing temporary live parameter state {}",
                temporary.display()
            )
        })?;
        drop(file);
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("replacing live parameter state {}", path.display()))?;
        }
        fs::rename(&temporary, path)
            .with_context(|| format!("committing live parameter state {}", path.display()))?;
        #[cfg(unix)]
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        self.dirty = false;
        Ok(())
    }
}

fn persistent_parameter(parameter: &rackforge_plugin_api::ParameterDescriptor) -> bool {
    !parameter.flags.read_only
        && !matches!(
            parameter.kind,
            ParameterKind::Trigger | ParameterKind::Meter { .. }
        )
}

#[cfg(not(target_arch = "wasm32"))]
mod writer {
    use super::LiveParameterStateStore;
    use rackforge_plugin_api::ParameterSchema;
    use std::sync::mpsc::{self, SyncSender, TrySendError};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    // More than thirty seconds of dense 240 Hz controller traffic. The worker
    // normally drains continuously; this headroom keeps a final fader value or
    // a program-reset marker from being sacrificed to a transient disk stall.
    const QUEUE_CAPACITY: usize = 8_192;
    const QUIET_FLUSH: Duration = Duration::from_millis(1500);
    const MAX_FLUSH: Duration = Duration::from_secs(5);

    #[derive(Clone)]
    pub struct LiveParameterTarget {
        pub plugin_id: String,
        pub plugin_version: String,
        pub schema: ParameterSchema,
    }

    enum Message {
        Record {
            target: usize,
            parameter_index: u32,
            value: f64,
        },
        Clear {
            target: usize,
        },
        Register {
            target: LiveParameterTarget,
            reply: SyncSender<usize>,
        },
        Flush(SyncSender<()>),
        Shutdown,
    }

    #[derive(Clone)]
    pub struct LiveParameterWriterHandle {
        sender: SyncSender<Message>,
    }

    impl LiveParameterWriterHandle {
        /// Real-time-safe best effort: the audio path never allocates, locks,
        /// blocks, or touches the filesystem.
        pub fn try_record(&self, target: usize, parameter_index: u32, value: f64) {
            if let Err(TrySendError::Disconnected(_)) = self.sender.try_send(Message::Record {
                target,
                parameter_index,
                value,
            }) {
                eprintln!("LIVE_PARAMETER_WRITER_DISCONNECTED");
            }
        }

        pub fn clear(&self, target: usize) {
            let _ = self.sender.try_send(Message::Clear { target });
        }

        pub fn flush(&self) {
            let (sender, receiver) = mpsc::sync_channel(0);
            if self.sender.send(Message::Flush(sender)).is_ok() {
                let _ = receiver.recv();
            }
        }

        pub fn register(&self, target: LiveParameterTarget) -> Option<usize> {
            let (reply, receiver) = mpsc::sync_channel(0);
            self.sender.send(Message::Register { target, reply }).ok()?;
            receiver.recv().ok()
        }
    }

    pub struct LiveParameterWriter {
        handle: LiveParameterWriterHandle,
        join: Option<JoinHandle<()>>,
    }

    impl LiveParameterWriter {
        pub fn start(store: LiveParameterStateStore, targets: Vec<LiveParameterTarget>) -> Self {
            let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
            let handle = LiveParameterWriterHandle {
                sender: sender.clone(),
            };
            let join = thread::Builder::new()
                .name("rackforge-live-state".to_owned())
                .spawn(move || {
                    let mut store = store;
                    let mut targets = targets;
                    let mut last_change: Option<Instant> = None;
                    let mut first_change: Option<Instant> = None;
                    loop {
                        let message = receiver.recv_timeout(Duration::from_millis(100));
                        match message {
                            Ok(Message::Record { target, parameter_index, value }) => {
                                if let Some(target) = targets.get(target) {
                                    match store.record(
                                        &target.plugin_id,
                                        &target.plugin_version,
                                        &target.schema,
                                        parameter_index,
                                        value,
                                    ) {
                                        Ok(true) => {
                                            let now = Instant::now();
                                            first_change.get_or_insert(now);
                                            last_change = Some(now);
                                        }
                                        Ok(false) => {}
                                        Err(error) => eprintln!(
                                            "LIVE_PARAMETER_RECORD_REJECTED plugin={} parameter={} reason={error}",
                                            target.plugin_id, parameter_index
                                        ),
                                    }
                                }
                            }
                            Ok(Message::Clear { target }) => {
                                if let Some(target) = targets.get(target) {
                                    if store.clear_plugin(&target.plugin_id) {
                                        // Program selection is a baseline change, not a
                                        // high-rate control gesture. Commit it immediately so a
                                        // power loss cannot resurrect yesterday's overrides on
                                        // top of the newly selected program.
                                        flush(&mut store);
                                    }
                                    last_change = None;
                                    first_change = None;
                                }
                            }
                            Ok(Message::Register { target, reply }) => {
                                let index = targets.len();
                                targets.push(target);
                                let _ = reply.send(index);
                            }
                            Ok(Message::Flush(reply)) => {
                                flush(&mut store);
                                last_change = None;
                                first_change = None;
                                let _ = reply.send(());
                            }
                            Ok(Message::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                                flush(&mut store);
                                break;
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                        let now = Instant::now();
                        if last_change.is_some_and(|changed| now.duration_since(changed) >= QUIET_FLUSH)
                            || first_change
                                .is_some_and(|changed| now.duration_since(changed) >= MAX_FLUSH)
                        {
                            flush(&mut store);
                            last_change = None;
                            first_change = None;
                        }
                    }
                })
                .expect("spawning live parameter persistence worker");
            Self {
                handle,
                join: Some(join),
            }
        }

        pub fn handle(&self) -> LiveParameterWriterHandle {
            self.handle.clone()
        }
    }

    impl Drop for LiveParameterWriter {
        fn drop(&mut self) {
            let _ = self.handle.sender.send(Message::Shutdown);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    fn flush(store: &mut LiveParameterStateStore) {
        if let Err(error) = store.flush() {
            eprintln!("LIVE_PARAMETER_STATE_SAVE_FAILED reason={error:#}");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use writer::{LiveParameterTarget, LiveParameterWriter, LiveParameterWriterHandle};

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_plugin_api::ParameterTaper;
    use rackforge_plugin_api::{
        EnumChoice, PageDescriptor, ParameterDescriptor, ParameterFlags, SuggestedControl,
    };

    fn schema(index: u32) -> ParameterSchema {
        ParameterSchema {
            schema_version: 1,
            display_decimals: None,
            pages: vec![PageDescriptor {
                id: "main".into(),
                name: "Main".into(),
                order: 0,
                header: None,
            }],
            parameters: vec![
                ParameterDescriptor {
                    index,
                    id: "filter.cutoff".into(),
                    name: "Cutoff".into(),
                    page: "main".into(),
                    group: None,
                    order: 0,
                    kind: ParameterKind::Float {
                        minimum: 0.0,
                        maximum: 1.0,
                        default: 0.5,
                        step: 0.01,
                        unit: None,
                        taper: ParameterTaper::Linear,
                    },
                    flags: ParameterFlags::default(),
                    suggested_control: SuggestedControl::Knob,
                },
                ParameterDescriptor {
                    index: 99,
                    id: "voice.mode".into(),
                    name: "Mode".into(),
                    page: "main".into(),
                    group: None,
                    order: 1,
                    kind: ParameterKind::Enum {
                        default: 0,
                        choices: vec![
                            EnumChoice {
                                value: 0,
                                name: "A".into(),
                            },
                            EnumChoice {
                                value: 2,
                                name: "B".into(),
                            },
                        ],
                    },
                    flags: ParameterFlags::default(),
                    suggested_control: SuggestedControl::List,
                },
            ],
            semantic_controls: Vec::new(),
        }
    }

    #[test]
    fn persists_and_restores_by_stable_parameter_id() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-live-parameter-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let mut store = LiveParameterStateStore::open(Some(&root)).unwrap();
        store
            .record("org.test.synth", "1.0.0", &schema(7), 7, 0.75)
            .unwrap();
        store.flush().unwrap();

        let reopened = LiveParameterStateStore::open(Some(&root)).unwrap();
        assert_eq!(
            reopened.restored_values("org.test.synth", &schema(42)),
            vec![(42, 0.75)]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_invalid_values_and_clears_plugin_overrides() {
        let mut store = LiveParameterStateStore::open(None).unwrap();
        assert!(
            store
                .record("org.test.synth", "1", &schema(7), 99, 1.0)
                .is_err()
        );
        store
            .record("org.test.synth", "1", &schema(7), 99, 2.0)
            .unwrap();
        assert_eq!(
            store.restored_values("org.test.synth", &schema(7)),
            vec![(99, 2.0)]
        );
        assert!(store.clear_plugin("org.test.synth"));
        assert!(
            store
                .restored_values("org.test.synth", &schema(7))
                .is_empty()
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn background_writer_coalesces_and_flushes_the_latest_value() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-live-parameter-writer-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = LiveParameterStateStore::open(Some(&root)).unwrap();
        let writer = LiveParameterWriter::start(
            store,
            vec![LiveParameterTarget {
                plugin_id: "org.test.synth".into(),
                plugin_version: "1.0.0".into(),
                schema: schema(7),
            }],
        );
        let handle = writer.handle();
        handle.try_record(0, 7, 0.25);
        handle.try_record(0, 7, 0.9);
        handle.flush();
        drop(writer);

        let reopened = LiveParameterStateStore::open(Some(&root)).unwrap();
        assert_eq!(
            reopened.restored_values("org.test.synth", &schema(7)),
            vec![(7, 0.9)]
        );
        let _ = fs::remove_dir_all(root);
    }
}
