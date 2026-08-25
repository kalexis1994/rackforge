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
        self.live_state_dirty.take()?;
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
                let rect = ui.max_rect();
                let painter = ui.painter();
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        rect.left_top(),
                        egui::pos2(rect.right(), rect.top() + 2.0),
                    ),
                    0.0,
                    Color32::from_rgb(55, 205, 222),
                );
                painter.circle_filled(
                    rect.center_top() + egui::vec2(0.0, 72.0),
                    170.0,
                    Color32::from_rgba_unmultiplied(18, 103, 124, 18),
                );
            });

        let modal_width = (context.screen_rect().width() - 48.0).clamp(320.0, 500.0);
        egui::Area::new(egui::Id::new("rackforge_shutdown_progress"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(7, 28, 39))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 105, 122)))
                    .corner_radius(18.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 2,
                        color: Color32::from_black_alpha(150),
                    })
                    .inner_margin(egui::Margin::symmetric(28, 26))
                    .show(ui, |ui| {
                        ui.set_width(modal_width);

                        ui.horizontal(|ui| {
                            egui::Frame::new()
                                .fill(Color32::from_rgb(86, 221, 232))
                                .corner_radius(11.0)
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("RF")
                                            .strong()
                                            .size(15.0)
                                            .color(Color32::from_rgb(4, 28, 38)),
                                    );
                                });
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("CLOSING RACKFORGE")
                                        .strong()
                                        .size(18.0)
                                        .color(Color32::from_rgb(225, 250, 252)),
                                );
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new("Finishing your session safely")
                                        .size(13.0)
                                        .color(Color32::from_rgb(133, 169, 181)),
                                );
                            });
                        });

                        ui.add_space(22.0);
                        let progress = self.step.index() as f32 / Step::ALL.len() as f32;
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(modal_width)
                                .desired_height(4.0)
                                .fill(Color32::from_rgb(86, 221, 232)),
                        );
                        ui.add_space(18.0);

                        for step in Step::ALL {
                            let completed = step.index() < self.step.index();
                            let active = step == self.step;
                            let row_fill = if active {
                                Color32::from_rgb(10, 42, 54)
                            } else {
                                Color32::TRANSPARENT
                            };
                            let row_stroke = if active {
                                Stroke::new(1.0_f32, Color32::from_rgb(34, 100, 116))
                            } else {
                                Stroke::NONE
                            };

                            egui::Frame::new()
                                .fill(row_fill)
                                .stroke(row_stroke)
                                .corner_radius(9.0)
                                .inner_margin(egui::Margin::symmetric(12, 9))
                                .show(ui, |ui| {
                                    ui.set_width(modal_width - 24.0);
                                    ui.horizontal(|ui| {
                                        if active {
                                            ui.add(
                                                egui::Spinner::new()
                                                    .size(16.0)
                                                    .color(Color32::from_rgb(86, 221, 232)),
                                            );
                                        } else {
                                            let (icon_rect, _) = ui.allocate_exact_size(
                                                egui::vec2(16.0, 16.0),
                                                egui::Sense::hover(),
                                            );
                                            let icon_color = if completed {
                                                Color32::from_rgb(86, 221, 232)
                                            } else {
                                                Color32::from_rgb(45, 74, 84)
                                            };
                                            ui.painter().circle_stroke(
                                                icon_rect.center(),
                                                6.0,
                                                Stroke::new(1.5_f32, icon_color),
                                            );
                                            if completed {
                                                ui.painter().circle_filled(
                                                    icon_rect.center(),
                                                    3.0,
                                                    icon_color,
                                                );
                                            }
                                        }
                                        ui.add_space(8.0);
                                        ui.label(RichText::new(step.label()).size(13.5).color(
                                            if active {
                                                Color32::from_rgb(226, 249, 251)
                                            } else if completed {
                                                Color32::from_rgb(153, 190, 199)
                                            } else {
                                                Color32::from_rgb(91, 121, 132)
                                            },
                                        ));
                                        if active {
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new("WORKING")
                                                            .strong()
                                                            .size(10.0)
                                                            .color(Color32::from_rgb(86, 221, 232)),
                                                    );
                                                },
                                            );
                                        }
                                    });
                                });
                            ui.add_space(4.0);
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Please keep RackForge open")
                                    .size(11.5)
                                    .color(Color32::from_rgb(102, 136, 147)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} / {}",
                                            self.step.index().min(Step::ALL.len()),
                                            Step::ALL.len()
                                        ))
                                        .size(11.5)
                                        .color(Color32::from_rgb(102, 136, 147)),
                                    );
                                },
                            );
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
