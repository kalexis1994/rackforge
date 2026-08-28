use super::{DesktopApp, live_state_path};
use eframe::egui::{self, Color32, RichText, Stroke};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

/// Panels take radius 3 and keys radius 2, the same as the interface. Nothing
/// in this design is a pill.
const PANEL_RADIUS: f32 = 3.0;
const KEY_RADIUS: f32 = 2.0;

/// The faceplate palette, as the interface defines it in `styles.css`. Kept as
/// literals rather than read from anywhere: this panel runs while the web stack
/// is being torn down, so it cannot depend on it for its own colours.
#[derive(Clone, Copy)]
struct Palette {
    chassis: Color32,
    paper: Color32,
    panel_inset: Color32,
    line: Color32,
    ink: Color32,
    muted: Color32,
    faint: Color32,
    acid: Color32,
    drop: Color32,
    glow: Color32,
    glow_peak: f32,
}

impl Palette {
    const DAYLIGHT: Self = Self {
        chassis: Color32::from_rgb(0xc9, 0xc1, 0xb3),
        paper: Color32::from_rgb(0xeb, 0xe4, 0xd8),
        panel_inset: Color32::from_rgb(0xde, 0xd6, 0xc7),
        line: Color32::from_rgb(0xbd, 0xb3, 0xa2),
        ink: Color32::from_rgb(0x22, 0x1e, 0x1a),
        muted: Color32::from_rgb(0x6e, 0x65, 0x55),
        faint: Color32::from_rgb(0x94, 0x8b, 0x79),
        acid: Color32::from_rgb(0x2f, 0x4b, 0x7c),
        drop: Color32::from_black_alpha(40),
        glow: Color32::from_rgb(0xff, 0xfd, 0xf7),
        glow_peak: 0.5,
    };

    const STAGE: Self = Self {
        chassis: Color32::from_rgb(0x13, 0x14, 0x17),
        paper: Color32::from_rgb(0x21, 0x23, 0x27),
        panel_inset: Color32::from_rgb(0x19, 0x1b, 0x1e),
        line: Color32::from_rgb(0x34, 0x38, 0x3e),
        ink: Color32::from_rgb(0xef, 0xe9, 0xde),
        muted: Color32::from_rgb(0x9d, 0x97, 0x8c),
        faint: Color32::from_rgb(0x6b, 0x67, 0x60),
        acid: Color32::from_rgb(0x7c, 0xa5, 0xe0),
        drop: Color32::from_black_alpha(120),
        glow: Color32::from_rgb(0x7c, 0xa5, 0xe0),
        glow_peak: 0.15,
    };

    fn for_context(context: &egui::Context) -> Self {
        if context.style().visuals.dark_mode {
            Self::STAGE
        } else {
            Self::DAYLIGHT
        }
    }
}

/// Paints the pool of light behind the panel.
///
/// Light does not fall off linearly, and a linear ramp reads as a disc with an
/// edge rather than as a glow. The profile here is a Gaussian,
/// `alpha(t) = peak · exp(-4.6·t²)`, which reaches effectively zero at the rim.
///
/// It is painted as filled discs from the outside in, and a disc laid over what
/// is already there does not add its alpha — it composites. So each step paints
/// the increment that lands on the target instead of the target itself:
///
/// ```text
///     a_k = (A_k - A_{k-1}) / (1 - A_{k-1})
/// ```
///
/// which is the same correction the CSS ramp makes with its stops. Without it
/// the middle of the pool piles up and the edge stays hard — exactly the
/// blotchy halo this replaced.
fn paint_glow(painter: &egui::Painter, center: egui::Pos2, radius: f32, palette: Palette) {
    let [r, g, b, _] = palette.glow.to_array();
    for (t, increment) in glow_discs(palette.glow_peak) {
        painter.circle_filled(
            center,
            radius * t,
            Color32::from_rgba_unmultiplied(r, g, b, (increment * 255.0).round() as u8),
        );
    }
}

/// The discs of [`paint_glow`], outside in, as `(radius fraction, own alpha)`.
///
/// Split out so the correction can be pinned by a test: written as the target
/// alpha per disc instead of the increment, the pool piles up in the middle and
/// keeps a hard rim, which is the artefact this exists to avoid.
fn glow_discs(peak: f32) -> Vec<(f32, f32)> {
    const STEPS: usize = 14;
    let mut discs = Vec::with_capacity(STEPS);
    let mut reached = 0.0_f32;
    for step in 0..STEPS {
        // t runs 1 → 0 as the discs shrink towards the centre.
        let t = 1.0 - step as f32 / STEPS as f32;
        let target = peak * (-4.6 * t * t).exp();
        let increment = ((target - reached) / (1.0 - reached)).clamp(0.0, 1.0);
        reached = target;
        if increment > 0.002 {
            discs.push((t, increment));
        }
    }
    discs
}

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
        // The lighting switch lives in the interface's own storage, which this
        // side of the process cannot read, so the closing panel follows the
        // system theme — the same thing the interface's AUTO setting does.
        let palette = Palette::for_context(context);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(palette.chassis))
            .show(context, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                // A seam, not a lit bar: red is reserved for LIVE, and this is
                // the machine closing, not performing.
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        rect.left_top(),
                        egui::pos2(rect.right(), rect.top() + 1.0),
                    ),
                    0.0,
                    palette.line,
                );
                paint_glow(painter, rect.center(), rect.height() * 0.62, palette);
            });

        let modal_width = (context.screen_rect().width() - 48.0).clamp(320.0, 500.0);
        egui::Area::new(egui::Id::new("rackforge_shutdown_progress"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                egui::Frame::new()
                    .fill(palette.paper)
                    .stroke(Stroke::new(1.0_f32, palette.line))
                    .corner_radius(PANEL_RADIUS)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 3],
                        blur: 12,
                        spread: 0,
                        color: palette.drop,
                    })
                    .inner_margin(egui::Margin::symmetric(24, 22))
                    .show(ui, |ui| {
                        ui.set_width(modal_width);

                        ui.horizontal(|ui| {
                            // A moulded plate carrying the monogram, not a lit
                            // chip: nothing on a closing machine is powered.
                            egui::Frame::new()
                                .fill(palette.panel_inset)
                                .stroke(Stroke::new(1.0_f32, palette.line))
                                .corner_radius(KEY_RADIUS)
                                .inner_margin(egui::Margin::symmetric(9, 7))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("RF")
                                            .strong()
                                            .size(15.0)
                                            .color(palette.ink),
                                    );
                                });
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("CLOSING RACKFORGE")
                                        .strong()
                                        .size(17.0)
                                        .color(palette.ink),
                                );
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new("Finishing your session safely")
                                        .size(12.5)
                                        .color(palette.muted),
                                );
                            });
                        });

                        ui.add_space(20.0);
                        let progress = self.step.index() as f32 / Step::ALL.len() as f32;
                        ui.visuals_mut().extreme_bg_color = palette.panel_inset;
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(modal_width)
                                .desired_height(4.0)
                                .corner_radius(1.0)
                                .fill(palette.acid),
                        );
                        ui.add_space(16.0);

                        for step in Step::ALL {
                            let completed = step.index() < self.step.index();
                            let active = step == self.step;
                            // The running step is the engaged one, so it is the
                            // one that sits in a recess — same reading as every
                            // engaged control in the interface.
                            let row_fill = if active {
                                palette.panel_inset
                            } else {
                                Color32::TRANSPARENT
                            };
                            let row_stroke = if active {
                                Stroke::new(1.0_f32, palette.line)
                            } else {
                                Stroke::NONE
                            };

                            egui::Frame::new()
                                .fill(row_fill)
                                .stroke(row_stroke)
                                .corner_radius(KEY_RADIUS)
                                .inner_margin(egui::Margin::symmetric(12, 9))
                                .show(ui, |ui| {
                                    ui.set_width(modal_width - 24.0);
                                    ui.horizontal(|ui| {
                                        if active {
                                            ui.add(
                                                egui::Spinner::new()
                                                    .size(16.0)
                                                    .color(palette.acid),
                                            );
                                        } else {
                                            let (icon_rect, _) = ui.allocate_exact_size(
                                                egui::vec2(16.0, 16.0),
                                                egui::Sense::hover(),
                                            );
                                            let icon_color = if completed {
                                                palette.acid
                                            } else {
                                                palette.line
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
                                        ui.label(RichText::new(step.label()).size(13.0).color(
                                            if active {
                                                palette.ink
                                            } else if completed {
                                                palette.muted
                                            } else {
                                                palette.faint
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
                                                            .color(palette.acid),
                                                    );
                                                },
                                            );
                                        }
                                    });
                                });
                            ui.add_space(4.0);
                        }

                        ui.add_space(10.0);
                        ui.visuals_mut().widgets.noninteractive.bg_stroke =
                            Stroke::new(1.0_f32, palette.line);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Please keep RackForge open")
                                    .size(11.5)
                                    .color(palette.muted),
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
                                        .color(palette.muted),
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
    fn glow_discs_composite_onto_the_gaussian() {
        let peak = 0.5_f32;
        let discs = glow_discs(peak);
        assert!(discs.len() > 6, "the ramp needs enough steps to read as light");

        // Compositing the discs outside in has to land on alpha(t) = peak·e^(-4.6t²)
        // at every radius, which is the whole point of using the increment.
        let mut reached = 0.0_f32;
        for (t, increment) in discs {
            reached += increment * (1.0 - reached);
            let expected = peak * (-4.6 * t * t).exp();
            assert!(
                (reached - expected).abs() < 0.01,
                "at t={t} composited {reached} but wanted {expected}",
            );
        }

        // The profile is sampled, so the centre approaches the peak without
        // reaching it — the innermost disc still has a radius. What matters is
        // that it never overshoots, which would blow out the middle of the pool.
        assert!(reached <= peak, "centre overshot the peak: {reached}");
        assert!(reached > peak * 0.95, "centre fell short of the peak: {reached}");
    }

    #[test]
    fn glow_stays_inside_the_alpha_range() {
        for peak in [0.15_f32, 0.5] {
            for (t, increment) in glow_discs(peak) {
                assert!((0.0..=1.0).contains(&t));
                assert!((0.0..=1.0).contains(&increment));
            }
        }
    }

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
