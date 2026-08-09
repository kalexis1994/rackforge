use anyhow::{Context, Result, bail};
use rackforge_session_api::{LivePerformanceState, SessionId, SessionState, SurfaceMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const CHECKPOINT_SCHEMA_VERSION: u32 = 3;
const CHECKPOINT_DIRECTORY: &str = "sessions";
const LIVE_CHECKPOINT_FILE: &str = "live.main.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCheckpoint {
    schema_version: u32,
    session_id: String,
    active_mode: SurfaceMode,
    #[serde(default)]
    master_level: rackforge_session_api::MasterLevel,
    #[serde(default)]
    master_pan: rackforge_session_api::MasterPan,
    #[serde(default)]
    live: LivePerformanceState,
    #[serde(default)]
    active_instance_id: Option<String>,
    #[serde(default)]
    selected_sounds: BTreeMap<String, String>,
}

impl SessionCheckpoint {
    fn from_state(state: &SessionState) -> Self {
        Self {
            schema_version: CHECKPOINT_SCHEMA_VERSION,
            session_id: state.session_id.as_str().to_owned(),
            active_mode: state.active_mode,
            master_level: state.master_level,
            master_pan: state.master_pan,
            live: state.live.clone(),
            active_instance_id: state
                .active_instance_id
                .as_ref()
                .map(|instance_id| instance_id.as_str().to_owned()),
            selected_sounds: state
                .instances
                .iter()
                .filter_map(|instance| {
                    instance.selected_sound_id.as_ref().map(|sound_id| {
                        (instance.instance_id.as_str().to_owned(), sound_id.clone())
                    })
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionCheckpointStore {
    path: PathBuf,
}

impl SessionCheckpointStore {
    pub fn live(data_root: &Path) -> Self {
        Self {
            path: data_root
                .join(CHECKPOINT_DIRECTORY)
                .join(LIVE_CHECKPOINT_FILE),
        }
    }

    pub fn active_mode(&self, session_id: &SessionId) -> Result<Option<SurfaceMode>> {
        Ok(self
            .load(session_id)?
            .map(|checkpoint| checkpoint.active_mode))
    }

    pub fn active_instance_id(&self, session_id: &SessionId) -> Result<Option<String>> {
        Ok(self
            .load(session_id)?
            .and_then(|checkpoint| checkpoint.active_instance_id))
    }

    pub fn selected_sound(
        &self,
        session_id: &SessionId,
        instance_id: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .load(session_id)?
            .and_then(|checkpoint| checkpoint.selected_sounds.get(instance_id).cloned()))
    }

    pub fn master_level(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<rackforge_session_api::MasterLevel>> {
        Ok(self
            .load(session_id)?
            .map(|checkpoint| checkpoint.master_level))
    }

    pub fn master_pan(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<rackforge_session_api::MasterPan>> {
        Ok(self
            .load(session_id)?
            .map(|checkpoint| checkpoint.master_pan))
    }

    pub fn live_state(&self, session_id: &SessionId) -> Result<Option<LivePerformanceState>> {
        Ok(self
            .load(session_id)?
            .filter(|checkpoint| checkpoint.schema_version >= 2)
            .map(|checkpoint| checkpoint.live))
    }

    pub fn save(&self, state: &SessionState) -> Result<()> {
        let parent = self
            .path
            .parent()
            .context("session checkpoint path has no parent")?;
        fs::create_dir_all(parent).with_context(|| {
            format!("creating session checkpoint directory {}", parent.display())
        })?;

        let checkpoint = SessionCheckpoint::from_state(state);
        let bytes =
            serde_json::to_vec_pretty(&checkpoint).context("serializing session checkpoint")?;
        let temporary = self.path.with_extension("json.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .with_context(|| {
                format!(
                    "opening temporary session checkpoint {}",
                    temporary.display()
                )
            })?;
        file.write_all(&bytes)
            .context("writing temporary session checkpoint")?;
        file.write_all(b"\n")
            .context("terminating temporary session checkpoint")?;
        file.sync_all()
            .context("syncing temporary session checkpoint")?;
        drop(file);
        fs::rename(&temporary, &self.path).with_context(|| {
            format!(
                "atomically replacing session checkpoint {}",
                self.path.display()
            )
        })?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| {
                format!("syncing session checkpoint directory {}", parent.display())
            })?;
        Ok(())
    }

    fn load(&self, session_id: &SessionId) -> Result<Option<SessionCheckpoint>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("reading session checkpoint {}", self.path.display())
                });
            }
        };
        let checkpoint: SessionCheckpoint = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing session checkpoint {}", self.path.display()))?;
        if !matches!(checkpoint.schema_version, 1 | 2 | CHECKPOINT_SCHEMA_VERSION) {
            bail!(
                "unsupported session checkpoint schema {} in {}",
                checkpoint.schema_version,
                self.path.display()
            );
        }
        if checkpoint.session_id != session_id.as_str() {
            bail!(
                "checkpoint session {:?} does not match {:?}",
                checkpoint.session_id,
                session_id.as_str()
            );
        }
        Ok(Some(checkpoint))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_session_api::{
        DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, InstanceId, PluginInstanceState,
        Revision, SESSION_SCHEMA_VERSION, SoundSummary,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn session(mode: SurfaceMode, selected: &str) -> SessionState {
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id: SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            revision: Revision::ZERO,
            active_mode: mode,
            master_level: rackforge_session_api::MasterLevel::new(725).unwrap(),
            master_pan: rackforge_session_api::MasterPan::new(-250).unwrap(),
            live: rackforge_session_api::LivePerformanceState::default(),
            active_instance_id: Some(instance_id.clone()),
            instances: vec![PluginInstanceState {
                instance_id,
                plugin_id: "org.rackforge.rf-dls".into(),
                plugin_name: "RF-DLS".into(),
                ui_layouts: vec!["little@1".into()],
                config_available: true,
                banks: Vec::new(),
                sounds: vec![SoundSummary {
                    id: selected.into(),
                    name: "Test".into(),
                    bank: None,
                    detail: None,
                    category: None,
                    tags: Vec::new(),
                    editable: false,
                }],
                selected_sound_id: Some(selected.into()),
            }],
            audition: None,
            program_draft: None,
        }
    }

    fn temporary_data_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rackforge-checkpoint-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn round_trips_only_stable_session_context() {
        let root = temporary_data_root("round-trip");
        let store = SessionCheckpointStore::live(&root);
        let state = session(SurfaceMode::Play, "custom.user.stage-piano");

        store.save(&state).unwrap();

        assert_eq!(
            store.active_mode(&state.session_id).unwrap(),
            Some(SurfaceMode::Play)
        );
        assert_eq!(
            store.master_level(&state.session_id).unwrap(),
            Some(rackforge_session_api::MasterLevel::new(725).unwrap())
        );
        assert_eq!(
            store.master_pan(&state.session_id).unwrap(),
            Some(rackforge_session_api::MasterPan::new(-250).unwrap())
        );
        assert_eq!(
            store
                .selected_sound(&state.session_id, DEFAULT_LIVE_INSTANCE_ID)
                .unwrap()
                .as_deref(),
            Some("custom.user.stage-piano")
        );
        assert!(!store.path.with_extension("json.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_checkpoint_is_a_clean_first_start() {
        let root = temporary_data_root("missing");
        let store = SessionCheckpointStore::live(&root);
        let session_id = SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap();

        assert_eq!(store.active_mode(&session_id).unwrap(), None);
        assert_eq!(
            store
                .selected_sound(&session_id, DEFAULT_LIVE_INSTANCE_ID)
                .unwrap(),
            None
        );
    }
}
