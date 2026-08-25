use super::{DesktopApp, live_state_path};
use eframe::egui::{self, Color32, RichText, Stroke};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

const STATE_TIMEOUT: Duration = Duration::from_secs(1);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    SavingPluginState,
    StoppingAudioMidi,
    RestoringControllers,
    StoppingWebServices,
    Complete,
}

impl Step {
    const ALL: [Self; 4] = [
        Self::SavingPluginState,
        Self::StoppingAudioMidi,
        Self::RestoringControllers,
        Self::StoppingWebServices,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::SavingPluginState => "Saving plugin state",
            Self::StoppingAudioMidi => "Stopping audio and MIDI",
            Self::RestoringControllers => "Restoring controller display and lights",
            Self::StoppingWebServices => "Stopping Web services",
            Self::Complete => "Ready to close",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::SavingPluginState => 0,
            Self::StoppingAudioMidi => 1,
            Self::RestoringControllers => 2,
            Self::StoppingWebServices => 3,
            Self::Complete => 4,
        }
    }
}

#[cfg(windows)]
struct PendingLiveStateSave {
    response: Receiver<std::result::Result<Vec<u8>, String>>,
    path: PathBuf,
    deadline: Instant,
}

pub(super) struct DesktopShutdown {
    step: Step,
    started_at: Instant,
    cleanup_deadline: Option<Instant>,
    #[cfg(windows)]
    state_save: Option<PendingLiveStateSave>,
    #[cfg(windows)]
    audio_cleanup: Option<thread::JoinHandle<()>>,
    controller_cleanup: Option<thread::JoinHandle<()>>,
    warnings: Vec<String>,
}

impl DesktopApp {
    #[cfg(windows)]
    fn begin_shutdown_state_save(&mut self) -> Option<PendingLiveStateSave> {
        if self.live_state_dirty.take().is_none() {
            return None;
        }
        let audio = self.audio.as_ref()?;
        let active = self
            .session
            .read()
            .expect("session lock poisoned")
            .active_instance_id
            .clone()?;
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.instance_id == active.as_str())?;
        match audio.begin_save_active_state() {
            Ok(response) => Some(PendingLiveStateSave {
                response,
                path: live_state_path(&self.live_state_dir(), &plugin.plugin_id),
                deadline: Instant::now() + STATE_TIMEOUT,
            }),
            Err(error) => {
                eprintln!("DESKTOP_SHUTDOWN_STATE_REQUEST_FAILED {error:#}");
                None
            }
        }
    }
}

impl DesktopShutdown {
    pub(super) fn begin(app: &mut DesktopApp) -> Self {
        eprintln!("DESKTOP_SHUTDOWN_BEGIN");
        #[cfg(windows)]
        let state_save = app.begin_shutdown_state_save();
        let mut shutdown = Self {
            step: Step::SavingPluginState,
            started_at: Instant::now(),
            cleanup_deadline: None,
            #[cfg(windows)]
            state_save,
            #[cfg(windows)]
            audio_cleanup: None,
            controller_cleanup: None,
            warnings: Vec::new(),
        };
        #[cfg(windows)]
        if shutdown.state_save.is_none() {
            shutdown.start_cleanup(app);
        }
        #[cfg(not(windows))]
        shutdown.start_cleanup(app);
        shutdown
    }

    fn start_cleanup(&mut self, app: &mut DesktopApp) {
        self.step = Step::StoppingAudioMidi;
        self.cleanup_deadline = Some(Instant::now() + CLEANUP_TIMEOUT);
        app.web_servers.set_injected_midi(None);
        app.web_servers.request_shutdown();
        if let Some(shutdown) = &app.controller_shutdown {
            shutdown.store(true, std::sync::atomic::Ordering::Release);
        }
        self.controller_cleanup = app.controller_supervisor.take();
        #[cfg(windows)]
        if let Some(audio) = app.audio.take() {
            match thread::Builder::new()
                .name("rackforge-desktop-audio-shutdown".into())
                .spawn(move || drop(audio))
            {
                Ok(worker) => self.audio_cleanup = Some(worker),
                Err(error) => self
                    .warnings
                    .push(format!("Audio cleanup could not start: {error}")),
            }
        }
    }

    #[cfg(windows)]
    fn poll_state_save(&mut self) -> bool {
        let Some(pending) = self.state_save.as_ref() else {
            return true;
        };
        match pending.response.try_recv() {
            Ok(Ok(bytes)) => {
                if let Some(parent) = pending.path.parent()
                    && let Err(error) = fs::create_dir_all(parent)
                {
                    self.warnings
                        .push(format!("Plugin state directory was not saved: {error}"));
                } else if let Err(error) = fs::write(&pending.path, bytes) {
                    self.warnings
                        .push(format!("Plugin state was not saved: {error}"));
                }
                self.state_save = None;
                true
            }
            Ok(Err(error)) => {
                self.warnings
                    .push(format!("Plugin state was not saved: {error}"));
                self.state_save = None;
                true
            }
            Err(TryRecvError::Disconnected) => {
                self.warnings
                    .push("The audio engine closed before saving plugin state.".into());
                self.state_save = None;
                true
            }
            Err(TryRecvError::Empty) if Instant::now() >= pending.deadline => {
                self.warnings
                    .push("Plugin state save exceeded the shutdown deadline.".into());
                self.state_save = None;
                true
            }
            Err(TryRecvError::Empty) => false,
        }
    }

    fn deadline_reached(&self) -> bool {
        self.cleanup_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub(super) fn poll(&mut self, app: &mut DesktopApp) {
        match self.step {
            Step::SavingPluginState => {
                #[cfg(windows)]
                let complete = self.poll_state_save();
                #[cfg(not(windows))]
                let complete = true;
                if complete {
                    self.start_cleanup(app);
                }
            }
            Step::StoppingAudioMidi => {
                #[cfg(windows)]
                {
                    let finished = self
                        .audio_cleanup
                        .as_ref()
                        .is_none_or(thread::JoinHandle::is_finished);
                    if finished || self.deadline_reached() {
                        if let Some(worker) = self.audio_cleanup.take()
                            && worker.is_finished()
                            && worker.join().is_err()
                        {
                            self.warnings.push("Audio cleanup failed.".into());
                        }
                        if !finished {
                            self.warnings
                                .push("Audio cleanup exceeded the shutdown deadline.".into());
                        }
                        self.step = Step::RestoringControllers;
                    }
                }
                #[cfg(not(windows))]
                {
                    self.step = Step::RestoringControllers;
                }
            }
            Step::RestoringControllers => {
                let finished = self
                    .controller_cleanup
                    .as_ref()
                    .is_none_or(thread::JoinHandle::is_finished);
                if finished || self.deadline_reached() {
                    if let Some(worker) = self.controller_cleanup.take()
                        && worker.is_finished()
                        && worker.join().is_err()
                    {
                        self.warnings.push("Controller cleanup failed.".into());
                    }
                    if !finished {
                        self.warnings
                            .push("Controller cleanup exceeded the shutdown deadline.".into());
                    }
                    self.step = Step::StoppingWebServices;
                }
            }
            Step::StoppingWebServices => {
                let finished = app.web_servers.shutdown_complete();
                if finished || self.deadline_reached() {
                    app.web_servers.finish_shutdown(finished);
                    if !finished {
                        self.warnings
                            .push("Web shutdown exceeded the shutdown deadline.".into());
                    }
                    self.step = Step::Complete;
                    eprintln!(
                        "DESKTOP_SHUTDOWN_COMPLETE elapsed_ms={} warnings={}",
                        self.started_at.elapsed().as_millis(),
                        self.warnings.len()
                    );
                }
            }
            Step::Complete => {}
        }
    }

    pub(super) fn is_complete(&self) -> bool {
        self.step == Step::Complete
    }

    pub(super) fn render(&self, context: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(4, 18, 27)))
            .show(context, |ui| {
                ui.centered_and_justified(|ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(7, 28, 39))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 92, 110)))
                        .corner_radius(14.0)
                        .inner_margin(egui::Margin::symmetric(30, 26))
                        .show(ui, |ui| {
                            ui.set_min_width(430.0);
                            ui.vertical(|ui| {
                                ui.heading(
                                    RichText::new("CLOSING RACKFORGE")
                                        .color(Color32::from_rgb(216, 247, 252)),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("Finishing the current session safely…")
                                        .color(Color32::from_rgb(135, 166, 178)),
                                );
                                ui.add_space(20.0);
                                for step in Step::ALL {
                                    ui.horizontal(|ui| {
                                        if step.index() < self.step.index() {
                                            ui.colored_label(Color32::from_rgb(86, 221, 232), "✓");
                                        } else if step == self.step {
                                            ui.spinner();
                                        } else {
                                            ui.colored_label(Color32::from_rgb(62, 91, 102), "○");
                                        }
                                        ui.label(RichText::new(step.label()).color(
                                            if step == self.step {
                                                Color32::from_rgb(222, 248, 251)
                                            } else {
                                                Color32::from_rgb(125, 153, 164)
                                            },
                                        ));
                                    });
                                    ui.add_space(8.0);
                                }
                            });
                        });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_ordered_and_user_facing() {
        assert_eq!(Step::ALL.len(), 4);
        for (index, step) in Step::ALL.into_iter().enumerate() {
            assert_eq!(step.index(), index);
            assert!(!step.label().trim().is_empty());
        }
        assert_eq!(Step::Complete.index(), Step::ALL.len());
    }
}
