use crate::desktop_audio::{AudioInventory, AudioPreferences};
use eframe::egui::{
    self, Align, Color32, ComboBox, Key, Layout, RichText, ScrollArea, Stroke, Vec2,
};

pub enum AudioSettingsAction {
    None,
    Close,
    Apply(AudioPreferences),
    Rescan,
    TestNote,
}

pub struct AudioSettingsState {
    inventory: AudioInventory,
    applied: AudioPreferences,
    draft: AudioPreferences,
    notice: Option<(bool, String)>,
}

impl AudioSettingsState {
    pub fn new(inventory: AudioInventory, preferences: AudioPreferences) -> Self {
        Self {
            inventory,
            applied: preferences.clone(),
            draft: preferences,
            notice: None,
        }
    }

    pub fn replace_inventory(&mut self, inventory: AudioInventory) {
        self.inventory = inventory;
        self.normalize();
        self.notice = Some((true, "Audio and MIDI devices rescanned".into()));
    }

    pub fn applied(&mut self, preferences: AudioPreferences, message: String) {
        self.applied = preferences.clone();
        self.draft = preferences;
        self.notice = Some((true, message));
    }

    pub fn error(&mut self, message: String) {
        self.notice = Some((false, message));
    }

    pub fn show(&mut self, context: &egui::Context) -> AudioSettingsAction {
        let mut action = AudioSettingsAction::None;
        if context.input(|input| input.key_pressed(Key::Escape)) {
            action = AudioSettingsAction::Close;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(5, 15, 22)))
            .show(context, |ui| {
                let width = ui.available_width().min(920.0);
                let left_margin = ((ui.available_width() - width) * 0.5).max(0.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(left_margin);
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, ui.available_height()),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(width);
                            ui.add_space(26.0);
                            if ui.button("← Back to RackForge").clicked() {
                                action = AudioSettingsAction::Close;
                            }
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.heading(
                                        RichText::new("Audio & MIDI")
                                            .size(30.0)
                                            .color(Color32::from_rgb(226, 242, 245)),
                                    );
                                    ui.label(
                                        RichText::new(
                                            "Configure the desktop audio engine and controllers",
                                        )
                                        .color(Color32::from_rgb(126, 151, 160)),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("Close").clicked() {
                                        action = AudioSettingsAction::Close;
                                    }
                                });
                            });
                            ui.add_space(18.0);

                            ScrollArea::vertical().show(ui, |ui| {
                                ui.set_width(width);
                                self.audio_card(ui);
                                ui.add_space(14.0);
                                self.midi_card(ui);
                                ui.add_space(14.0);
                                self.status_card(ui);
                                ui.add_space(18.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Rescan devices").clicked() {
                                        action = AudioSettingsAction::Rescan;
                                    }
                                    if ui.button("Test note").clicked() {
                                        action = AudioSettingsAction::TestNote;
                                    }
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let valid = self.inventory.validate(&self.draft).is_ok();
                                        let changed = self.draft != self.applied;
                                        if ui
                                            .add_enabled(
                                                valid && changed,
                                                egui::Button::new("Apply"),
                                            )
                                            .clicked()
                                        {
                                            action = AudioSettingsAction::Apply(self.draft.clone());
                                        }
                                        if changed && ui.button("Discard changes").clicked() {
                                            self.draft = self.applied.clone();
                                            self.notice = None;
                                        }
                                    });
                                });
                                ui.add_space(28.0);
                            });
                        },
                    );
                });
            });
        action
    }

    fn audio_card(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.heading(RichText::new("Audio output").color(accent()));
            ui.add_space(10.0);
            setting_row(ui, "Driver", |ui| {
                let previous = self.draft.driver.clone();
                ComboBox::from_id_salt("audio-driver")
                    .selected_text(&self.draft.driver)
                    .width(360.0)
                    .show_ui(ui, |ui| {
                        for driver in self.inventory.drivers.iter().filter(|driver| driver.available)
                        {
                            ui.selectable_value(
                                &mut self.draft.driver,
                                driver.name.clone(),
                                &driver.name,
                            );
                        }
                    });
                if previous != self.draft.driver {
                    self.select_default_output();
                }
            });
            if let Some(driver) = self
                .inventory
                .drivers
                .iter()
                .find(|driver| !driver.available && driver.name == "ASIO")
            {
                ui.horizontal(|ui| {
                    ui.add_space(160.0);
                    ui.label(
                        RichText::new(format!("ASIO unavailable — {}", driver.detail))
                            .small()
                            .color(Color32::from_rgb(146, 157, 163)),
                    );
                });
            }
            ui.add_space(6.0);
            setting_row(ui, "Output device", |ui| {
                let previous = self.draft.output_device.clone();
                ComboBox::from_id_salt("audio-output")
                    .selected_text(&self.draft.output_device)
                    .width(360.0)
                    .show_ui(ui, |ui| {
                        for output in self
                            .inventory
                            .outputs
                            .iter()
                            .filter(|output| output.driver == self.draft.driver)
                        {
                            let label = if output.is_default {
                                format!("{} (default)", output.name)
                            } else {
                                output.name.clone()
                            };
                            ui.selectable_value(
                                &mut self.draft.output_device,
                                output.name.clone(),
                                label,
                            );
                        }
                    });
                if previous != self.draft.output_device {
                    self.normalize_format();
                }
            });
            ui.add_space(6.0);
            let output = self.inventory.output(&self.draft).cloned();
            setting_row(ui, "Sample rate", |ui| {
                ComboBox::from_id_salt("audio-rate")
                    .selected_text(format!("{} Hz", self.draft.sample_rate_hz))
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        if let Some(output) = output.as_ref() {
                            for rate in &output.sample_rates {
                                ui.selectable_value(
                                    &mut self.draft.sample_rate_hz,
                                    *rate,
                                    format!("{rate} Hz"),
                                );
                            }
                        }
                    });
            });
            ui.add_space(6.0);
            setting_row(ui, "Buffer size", |ui| {
                let selected = self.draft.buffer_frames.map_or_else(
                    || "System default".into(),
                    |frames| format_buffer(frames, self.draft.sample_rate_hz),
                );
                ComboBox::from_id_salt("audio-buffer")
                    .selected_text(selected)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.draft.buffer_frames, None, "System default");
                        if let Some(output) = output.as_ref() {
                            for frames in &output.buffer_frames {
                                ui.selectable_value(
                                    &mut self.draft.buffer_frames,
                                    Some(*frames),
                                    format_buffer(*frames, self.draft.sample_rate_hz),
                                );
                            }
                        }
                    });
            });
            if let Some(output) = output {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "{} device channel(s); RackForge renders stereo and maps it safely to the output.",
                        output.channels
                    ))
                    .small()
                    .color(Color32::from_rgb(126, 151, 160)),
                );
            }
        });
    }

    fn midi_card(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.heading(RichText::new("MIDI inputs").color(accent()));
            ui.label(
                RichText::new("Only enabled inputs are opened by RackForge")
                    .small()
                    .color(Color32::from_rgb(126, 151, 160)),
            );
            ui.add_space(10.0);
            if self.inventory.midi_inputs.is_empty() {
                ui.label("No MIDI input devices found.");
            } else {
                for name in &self.inventory.midi_inputs {
                    let mut enabled = self.draft.midi_inputs.contains(name);
                    if ui.checkbox(&mut enabled, name).changed() {
                        if enabled {
                            self.draft.midi_inputs.push(name.clone());
                            self.draft.midi_inputs.sort();
                            self.draft.midi_inputs.dedup();
                        } else {
                            self.draft.midi_inputs.retain(|selected| selected != name);
                        }
                    }
                }
            }
        });
    }

    fn status_card(&self, ui: &mut egui::Ui) {
        if let Some((success, message)) = &self.notice {
            let color = if *success {
                Color32::from_rgb(100, 220, 181)
            } else {
                Color32::from_rgb(242, 119, 119)
            };
            egui::Frame::NONE
                .fill(Color32::from_rgba_premultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    22,
                ))
                .stroke(Stroke::new(1.0_f32, color))
                .corner_radius(12.0)
                .inner_margin(14.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(message).color(color));
                });
        }
    }

    fn normalize(&mut self) {
        if self.inventory.output(&self.draft).is_none() {
            if !self
                .inventory
                .drivers
                .iter()
                .any(|driver| driver.available && driver.name == self.draft.driver)
                && let Some(driver) = self
                    .inventory
                    .drivers
                    .iter()
                    .find(|driver| driver.available)
            {
                self.draft.driver = driver.name.clone();
            }
            self.select_default_output();
        } else {
            self.normalize_format();
        }
    }

    fn select_default_output(&mut self) {
        if let Some(output) = self
            .inventory
            .outputs
            .iter()
            .find(|output| output.driver == self.draft.driver && output.is_default)
            .or_else(|| {
                self.inventory
                    .outputs
                    .iter()
                    .find(|output| output.driver == self.draft.driver)
            })
        {
            self.draft.output_device = output.name.clone();
            self.draft.sample_rate_hz = output.default_sample_rate;
            self.draft.buffer_frames = None;
        }
    }

    fn normalize_format(&mut self) {
        if let Some(output) = self.inventory.output(&self.draft) {
            if !output.sample_rates.contains(&self.draft.sample_rate_hz) {
                self.draft.sample_rate_hz = output.default_sample_rate;
            }
            if self
                .draft
                .buffer_frames
                .is_some_and(|frames| !output.buffer_frames.contains(&frames))
            {
                self.draft.buffer_frames = None;
            }
        }
    }
}

fn setting_row(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(34.0);
        ui.add_sized([150.0, 28.0], egui::Label::new(label));
        content(ui);
    });
}

fn card() -> egui::Frame {
    egui::Frame::NONE
        .fill(Color32::from_rgba_premultiplied(17, 38, 49, 225))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 75, 87)))
        .corner_radius(16.0)
        .inner_margin(20.0)
}

fn accent() -> Color32 {
    Color32::from_rgb(92, 226, 245)
}

fn format_buffer(frames: u32, sample_rate: u32) -> String {
    let milliseconds = frames as f64 * 1_000.0 / sample_rate.max(1) as f64;
    format!("{frames} frames · {milliseconds:.1} ms")
}
