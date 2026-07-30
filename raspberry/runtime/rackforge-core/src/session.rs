use anyhow::{Context, Result, anyhow, bail};
use rackforge_session_api::{
    CommandRef, EventEnvelope, Revision, SESSION_SCHEMA_VERSION, SessionEvent, SessionState,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const DEFAULT_EVENT_HISTORY_CAPACITY: usize = 256;

pub type SharedSessionStore = Arc<Mutex<SessionStore>>;

pub struct SessionStore {
    state: SessionState,
    events: VecDeque<EventEnvelope>,
    event_capacity: usize,
}

impl SessionStore {
    pub fn new(state: SessionState) -> Result<Self> {
        Self::with_capacity(state, DEFAULT_EVENT_HISTORY_CAPACITY)
    }

    pub fn with_capacity(state: SessionState, event_capacity: usize) -> Result<Self> {
        if state.schema_version != SESSION_SCHEMA_VERSION {
            bail!(
                "unsupported session schema version {}",
                state.schema_version
            );
        }
        if event_capacity == 0 {
            bail!("session event history capacity must be positive");
        }
        Ok(Self {
            state,
            events: VecDeque::with_capacity(event_capacity),
            event_capacity,
        })
    }

    pub fn shared(state: SessionState) -> Result<SharedSessionStore> {
        Ok(Arc::new(Mutex::new(Self::new(state)?)))
    }

    pub fn snapshot(&self) -> SessionState {
        self.state.clone()
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn record(
        &mut self,
        command: Option<CommandRef>,
        event: SessionEvent,
    ) -> Result<EventEnvelope> {
        self.record_many(command, vec![event])?
            .pop()
            .context("record_many returned no event")
    }

    pub fn record_many(
        &mut self,
        command: Option<CommandRef>,
        events: Vec<SessionEvent>,
    ) -> Result<Vec<EventEnvelope>> {
        if events.is_empty() {
            bail!("at least one session event is required");
        }
        let mut staged = self.state.clone();
        let mut envelopes = Vec::with_capacity(events.len());
        for event in events {
            let envelope = EventEnvelope {
                schema_version: SESSION_SCHEMA_VERSION,
                revision: staged.revision.next().map_err(|message| anyhow!(message))?,
                command: command.clone(),
                event,
            };
            staged
                .apply(&envelope)
                .map_err(|message| anyhow!(message))?;
            envelopes.push(envelope);
        }
        self.state = staged;
        for envelope in &envelopes {
            if self.events.len() == self.event_capacity {
                self.events.pop_front();
            }
            self.events.push_back(envelope.clone());
        }
        Ok(envelopes)
    }

    pub fn events_after(&self, revision: Revision) -> Result<Vec<EventEnvelope>> {
        if revision > self.state.revision {
            bail!(
                "requested revision {} is newer than current revision {}",
                revision.get(),
                self.state.revision.get()
            );
        }
        if let Some(oldest) = self.events.front()
            && revision.next().map_err(|message| anyhow!(message))? < oldest.revision
        {
            bail!(
                "event history no longer contains revision {}",
                revision.get()
            );
        }
        Ok(self
            .events
            .iter()
            .filter(|event| event.revision > revision)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_session_api::{
        ClientId, DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, InstanceId,
        PluginInstanceState, SessionId, SoundSummary,
    };

    fn store(capacity: usize) -> SessionStore {
        let instance_id = InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).unwrap();
        let state = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id: SessionId::new(DEFAULT_LIVE_SESSION_ID).unwrap(),
            revision: Revision::ZERO,
            active_mode: rackforge_session_api::SurfaceMode::Live,
            master_level: rackforge_session_api::MasterLevel::UNITY,
            master_pan: rackforge_session_api::MasterPan::CENTER,
            active_instance_id: Some(instance_id.clone()),
            instances: vec![PluginInstanceState {
                instance_id,
                plugin_id: "org.rackforge.rf-dls".into(),
                plugin_name: "RF-DLS".into(),
                ui_layouts: vec!["little@1".into()],
                sounds: vec![
                    SoundSummary {
                        id: "piano".into(),
                        name: "Piano".into(),
                        bank: None,
                        detail: None,
                    },
                    SoundSummary {
                        id: "strings".into(),
                        name: "Strings".into(),
                        bank: None,
                        detail: None,
                    },
                ],
                selected_sound_id: Some("piano".into()),
            }],
            audition: None,
            program_draft: None,
        };
        SessionStore::with_capacity(state, capacity).unwrap()
    }

    #[test]
    fn records_monotonic_events_and_rebuilds_the_snapshot() {
        let mut store = store(4);
        let instance_id = store.state().active_instance_id.clone().unwrap();
        let event = store
            .record(
                Some(CommandRef {
                    client_id: ClientId::new("test.session").unwrap(),
                    command_id: 9,
                }),
                SessionEvent::SoundSelected {
                    instance_id,
                    sound_id: "strings".into(),
                },
            )
            .unwrap();
        assert_eq!(event.revision.get(), 1);
        assert_eq!(store.snapshot().revision.get(), 1);
        assert_eq!(store.events_after(Revision::ZERO).unwrap(), vec![event]);
    }

    #[test]
    fn reports_when_a_subscriber_falls_behind_history() {
        let mut store = store(1);
        let instance_id = store.state().active_instance_id.clone().unwrap();
        for sound_id in ["strings", "piano"] {
            store
                .record(
                    None,
                    SessionEvent::SoundSelected {
                        instance_id: instance_id.clone(),
                        sound_id: sound_id.into(),
                    },
                )
                .unwrap();
        }
        assert!(store.events_after(Revision::ZERO).is_err());
        assert_eq!(store.events_after(Revision::new(1)).unwrap().len(), 1);
    }

    #[test]
    fn record_many_is_atomic_when_a_later_event_is_invalid() {
        let mut store = store(4);
        let before = store.snapshot();
        let instance_id = before.active_instance_id.clone().unwrap();
        let result = store.record_many(
            None,
            vec![
                SessionEvent::SoundSelected {
                    instance_id: instance_id.clone(),
                    sound_id: "strings".into(),
                },
                SessionEvent::SoundSelected {
                    instance_id,
                    sound_id: "missing".into(),
                },
            ],
        );

        assert!(result.is_err());
        assert_eq!(store.snapshot(), before);
        assert!(store.events_after(Revision::ZERO).unwrap().is_empty());
    }
}
