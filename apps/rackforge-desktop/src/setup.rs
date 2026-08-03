use crate::paths::{self, DesktopPaths, RootChoice, RootMode};
use eframe::egui::{self, Align, Color32, Layout, RichText};
use std::path::{Path, PathBuf};

pub struct SetupState {
    default_root: PathBuf,
    executable_directory: PathBuf,
    portable_root: PathBuf,
    custom_root: String,
    error: Option<String>,
    result: Option<std::result::Result<DesktopPaths, String>>,
}

impl SetupState {
    pub fn new(default_root: PathBuf, executable_directory: PathBuf) -> Self {
        let portable_root = paths::portable_root(&executable_directory);
        Self {
            default_root,
            executable_directory,
            portable_root,
            custom_root: String::new(),
            error: None,
            result: None,
        }
    }

    fn select(&mut self, mode: RootMode, root: PathBuf) {
        match paths::persist_choice(
            &RootChoice { mode, root },
            &self.default_root,
            &self.executable_directory,
        ) {
            Ok(paths) => self.result = Some(Ok(paths)),
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    #[cfg(windows)]
    fn browse(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Choose RackForge data folder");
        if !self.custom_root.trim().is_empty() {
            dialog = dialog.set_directory(self.custom_root.trim());
        }
        if let Some(path) = dialog.pick_folder() {
            self.custom_root = path.display().to_string();
            self.error = None;
        }
    }

    #[cfg(not(windows))]
    fn browse(&mut self) {}

    fn location_card(
        ui: &mut egui::Ui,
        title: &str,
        detail: &str,
        path: &Path,
        action: &str,
    ) -> bool {
        let mut selected = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(18.0).strong());
                    ui.label(detail);
                    ui.monospace(path.display().to_string());
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    selected = ui.button(action).clicked();
                });
            });
        });
        selected
    }

    pub fn update(&mut self, context: &egui::Context) -> Option<anyhow::Result<DesktopPaths>> {
        egui::CentralPanel::default().show(context, |ui| {
            ui.add_space(18.0);
            ui.heading(
                RichText::new("WHERE SHOULD RACKFORGE KEEP ITS DATA?")
                    .color(Color32::from_rgb(58, 216, 224)),
            );
            ui.label(
                "Plugins, sessions and settings live under one RackForge Root. You can move the whole library without changing plugin packages.",
            );
            ui.add_space(18.0);

            let default_root = self.default_root.clone();
            if Self::location_card(
                ui,
                "Recommended",
                "Best for a normal Windows installation. No administrator access is required.",
                &default_root,
                "Use recommended",
            ) {
                self.select(RootMode::Standard, default_root);
            }

            ui.add_space(10.0);
            let portable_root = self.portable_root.clone();
            if Self::location_card(
                ui,
                "Portable",
                "Keeps RackForgeData beside the application, suitable for an external drive or ZIP distribution.",
                &portable_root,
                "Use portable",
            ) {
                self.select(RootMode::Portable, portable_root);
            }

            ui.add_space(16.0);
            ui.label(RichText::new("Custom location").size(18.0).strong());
            ui.label("Choose another disk or an existing RackForge library.");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.custom_root)
                        .hint_text("D:\\RackForgeData")
                        .desired_width(470.0),
                );
                if ui.button("Browse...").clicked() {
                    self.browse();
                }
                let custom = self.custom_root.trim().to_owned();
                if ui
                    .add_enabled(!custom.is_empty(), egui::Button::new("Use folder"))
                    .clicked()
                {
                    self.select(RootMode::Custom, PathBuf::from(custom));
                }
            });

            if let Some(error) = self.error.as_deref() {
                ui.add_space(14.0);
                ui.colored_label(Color32::from_rgb(235, 105, 105), error);
            }
        });

        self.result
            .take()
            .map(|result| result.map_err(anyhow::Error::msg))
    }
}
