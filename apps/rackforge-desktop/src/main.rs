#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod web;

use anyhow::{Context, Result, bail};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Layout, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Vec2,
};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage};
use rackforge_plugin_api::PluginKind;
use rackforge_session_api::{
    DEFAULT_LIVE_SESSION_ID, InstanceId, PluginInstanceState, Revision, SessionId, SessionState,
    SoundSummary,
};
use rackforge_surface_api::SurfaceMode;
use rackforge_surface_runtime::{
    ActiveMode, Header, Input, Menu, MenuCommand, PlayPlugin, PlaySound,
};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

const LONG_PRESS: Duration = Duration::from_millis(700);
const LITTLE_WIDTH: f32 = 760.0;
const LITTLE_HEIGHT: f32 = 270.0;

#[derive(Clone, Copy)]
struct LittleGeometry {
    outer: Rect,
    glass: Rect,
    header: Rect,
    footer: Rect,
    line_1: Pos2,
    line_2: Pos2,
    columns: [f32; 4],
}

impl LittleGeometry {
    fn new(outer: Rect) -> Self {
        let glass = outer.shrink2(Vec2::new(12.0, 12.0));
        let header = Rect::from_min_max(
            glass.min,
            Pos2::new(glass.max.x, glass.min.y + glass.height() * 0.22),
        );
        let footer = Rect::from_min_max(
            Pos2::new(glass.min.x, glass.max.y - glass.height() * 0.20),
            glass.max,
        );
        let body_height = footer.min.y - header.max.y;
        let column_width = glass.width() / 4.0;
        Self {
            outer,
            glass,
            header,
            footer,
            line_1: Pos2::new(glass.center().x, header.max.y + body_height * 0.39),
            line_2: Pos2::new(glass.center().x, header.max.y + body_height * 0.70),
            columns: std::array::from_fn(|index| glass.min.x + column_width * (index as f32 + 0.5)),
        }
    }
}

struct Options {
    port: u16,
    lan: bool,
    plugins_root: PathBuf,
    data_root: PathBuf,
}

struct DesktopPlugin {
    instance_id: String,
    plugin_id: String,
    name: String,
    config_available: bool,
    sounds: Vec<PlaySound>,
    selected_sound_id: Option<String>,
    instance: PluginInstance<'static>,
}

struct DesktopApp {
    menu: Menu,
    session: Arc<RwLock<SessionState>>,
    button_down: [Option<Instant>; 4],
    keyboard_down: [Option<Instant>; 4],
    web_url: String,
    status: String,
    plugins: Vec<DesktopPlugin>,
}

impl DesktopApp {
    fn new(session: Arc<RwLock<SessionState>>, options: &Options) -> Result<Self> {
        let (plugins, warnings) = load_desktop_plugins(&options.plugins_root, &options.data_root)?;
        let mut menu = Menu::default();
        let active_instance_id = plugins.first().map(|plugin| plugin.instance_id.as_str());
        menu.set_play_plugins(
            plugins
                .iter()
                .map(|plugin| {
                    PlayPlugin::new(&plugin.instance_id, &plugin.plugin_id, &plugin.name)
                        .config_available(plugin.config_available)
                })
                .collect(),
            active_instance_id,
        );
        if let Some(plugin) = plugins.first() {
            menu.set_active_plugin(&plugin.plugin_id, &plugin.name);
            menu.set_play_sounds(plugin.sounds.clone(), plugin.selected_sound_id.as_deref());
        }

        {
            let mut state = session.write().expect("session lock poisoned");
            state.active_instance_id = active_instance_id
                .map(InstanceId::new)
                .transpose()
                .map_err(anyhow::Error::msg)?;
            state.instances = plugins.iter().map(plugin_session_state).collect();
        }

        let status = if plugins.is_empty() {
            if warnings.is_empty() {
                format!("No plugins installed in {}", options.plugins_root.display())
            } else {
                warnings.join(" · ")
            }
        } else if warnings.is_empty() {
            format!("{} plugin(s) ready", plugins.len())
        } else {
            format!(
                "{} plugin(s) ready · {}",
                plugins.len(),
                warnings.join(" · ")
            )
        };

        Ok(Self {
            menu,
            session,
            button_down: [None; 4],
            keyboard_down: [None; 4],
            web_url: format!("http://127.0.0.1:{}", options.port),
            status,
            plugins,
        })
    }

    fn apply_input(&mut self, input: Input) {
        self.menu.apply_input(input);
        while let Some(command) = self.menu.take_command() {
            self.apply_command(command);
        }
    }

    fn apply_command(&mut self, command: MenuCommand) {
        match command {
            MenuCommand::SetActiveMode { mode } => {
                let surface_mode = match mode {
                    ActiveMode::Idle => SurfaceMode::Idle,
                    ActiveMode::Live => SurfaceMode::Live,
                    ActiveMode::Play => SurfaceMode::Play,
                };
                self.menu.sync_active_mode(mode);
                let mut session = self.session.write().expect("session lock poisoned");
                session.active_mode = surface_mode;
                session.revision = Revision::new(session.revision.get().saturating_add(1));
                self.status = format!("Active mode: {mode:?}");
            }
            MenuCommand::SelectPlugin { instance_id } => {
                let Some(index) = self
                    .plugins
                    .iter()
                    .position(|plugin| plugin.instance_id == instance_id)
                else {
                    self.status = format!("Unknown plugin instance: {instance_id}");
                    return;
                };
                let plugin = &self.plugins[index];
                self.menu.set_active_plugin(&plugin.plugin_id, &plugin.name);
                self.menu
                    .set_play_sounds(plugin.sounds.clone(), plugin.selected_sound_id.as_deref());
                let mut session = self.session.write().expect("session lock poisoned");
                session.active_instance_id = Some(
                    InstanceId::new(plugin.instance_id.clone()).expect("validated instance id"),
                );
                session.revision = Revision::new(session.revision.get().saturating_add(1));
                self.status = format!("{} selected", plugin.name);
            }
            MenuCommand::SelectSound { id } => {
                let Some(active_id) = self
                    .session
                    .read()
                    .expect("session lock poisoned")
                    .active_instance_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned())
                else {
                    self.status = "No active plugin".into();
                    return;
                };
                let Some(index) = self
                    .plugins
                    .iter()
                    .position(|plugin| plugin.instance_id == active_id)
                else {
                    self.status = format!("Unknown active plugin instance: {active_id}");
                    return;
                };
                match self.plugins[index].instance.load_preset(&id) {
                    Ok(()) => {
                        self.plugins[index].selected_sound_id = Some(id.clone());
                        let sounds = self.plugins[index].sounds.clone();
                        self.menu.set_play_sounds(sounds, Some(&id));
                        let mut session = self.session.write().expect("session lock poisoned");
                        if let Some(instance) = session
                            .instances
                            .iter_mut()
                            .find(|instance| instance.instance_id.as_str() == active_id)
                        {
                            instance.selected_sound_id = Some(id.clone());
                        }
                        session.revision = Revision::new(session.revision.get().saturating_add(1));
                        self.status = format!("Loaded {id}");
                    }
                    Err(error) => self.status = format!("Could not load {id}: {error:#}"),
                }
            }
            other => {
                self.status = format!("Desktop bridge pending: {other:?}");
            }
        }
    }

    fn keyboard(&mut self, context: &egui::Context) {
        let keys = [Key::Q, Key::W, Key::E, Key::R];
        for (index, key) in keys.into_iter().enumerate() {
            let down = context.input(|input| input.key_down(key));
            match (down, self.keyboard_down[index]) {
                (true, None) => {
                    self.keyboard_down[index] = Some(Instant::now());
                    self.menu.set_button_pressed(short_input(index), true);
                }
                (false, Some(started)) => {
                    self.keyboard_down[index] = None;
                    self.menu.set_button_pressed(short_input(index), false);
                    self.apply_input(if started.elapsed() >= LONG_PRESS {
                        long_input(index)
                    } else {
                        short_input(index)
                    });
                }
                _ => {}
            }
        }
        if context.input(|input| input.key_pressed(Key::ArrowLeft)) {
            self.apply_input(Input::Button2);
        }
        if context.input(|input| input.key_pressed(Key::ArrowRight)) {
            self.apply_input(Input::Button3);
        }
        if context.input(|input| input.key_pressed(Key::Enter)) {
            self.apply_input(Input::Button1);
        }
        if context.input(|input| input.key_pressed(Key::Escape)) {
            self.apply_input(Input::Button4);
        }
    }

    fn little_display(&mut self, ui: &mut egui::Ui) {
        let screen = self.menu.render();
        let width = ui.available_width().min(LITTLE_WIDTH);
        let (outer, _) = ui.allocate_exact_size(Vec2::new(width, LITTLE_HEIGHT), Sense::hover());
        let geometry = LittleGeometry::new(outer);
        let painter = ui.painter_at(geometry.outer);

        painter.rect_filled(
            geometry.outer.translate(Vec2::new(0.0, 5.0)),
            18.0,
            Color32::from_black_alpha(72),
        );
        painter.rect(
            geometry.outer,
            18.0,
            Color32::from_rgb(37, 43, 47),
            Stroke::new(1.5_f32, Color32::from_rgb(91, 101, 106)),
            StrokeKind::Inside,
        );

        let glass = painter.with_clip_rect(geometry.glass);
        glass.rect_filled(geometry.glass, 9.0, Color32::from_rgb(222, 228, 216));

        let mut scan_y = geometry.glass.min.y + 3.0;
        while scan_y < geometry.glass.max.y {
            glass.line_segment(
                [
                    Pos2::new(geometry.glass.min.x, scan_y),
                    Pos2::new(geometry.glass.max.x, scan_y),
                ],
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(42, 55, 45, 9)),
            );
            scan_y += 5.0;
        }

        glass.rect_filled(geometry.header, 0.0, Color32::from_rgb(16, 20, 22));
        if let Header::Visible(title) = &screen.header {
            glass.text(
                Pos2::new(geometry.header.min.x + 18.0, geometry.header.center().y),
                Align2::LEFT_CENTER,
                title,
                FontId::monospace(20.0),
                Color32::from_rgb(244, 247, 240),
            );
        }

        glass.text(
            geometry.line_1,
            Align2::CENTER_CENTER,
            &screen.line_1,
            FontId::monospace(30.0),
            Color32::from_rgb(10, 16, 14),
        );
        glass.text(
            geometry.line_2,
            Align2::CENTER_CENTER,
            &screen.line_2,
            FontId::monospace(22.0),
            Color32::from_rgb(45, 57, 50),
        );

        glass.rect_filled(
            geometry.footer,
            0.0,
            Color32::from_rgba_unmultiplied(94, 108, 96, 22),
        );
        glass.line_segment(
            [geometry.footer.left_top(), geometry.footer.right_top()],
            Stroke::new(1.0_f32, Color32::from_rgb(151, 162, 151)),
        );
        let button_width = geometry.footer.width() / 4.0;
        for index in 0..4 {
            let center = Pos2::new(geometry.columns[index], geometry.footer.center().y);
            if self.button_is_down(index) {
                let highlight = Rect::from_center_size(center, Vec2::new(button_width - 8.0, 34.0));
                glass.rect_filled(highlight, 5.0, Color32::from_rgb(16, 20, 22));
            }
            glass.text(
                center,
                Align2::CENTER_CENTER,
                &screen.footer[index].label,
                FontId::monospace(16.0),
                if self.button_is_down(index) {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(25, 30, 32)
                },
            );
        }

        painter.rect_stroke(
            geometry.glass,
            9.0,
            Stroke::new(2.0_f32, Color32::from_rgb(7, 10, 11)),
            StrokeKind::Inside,
        );
        painter.line_segment(
            [
                Pos2::new(geometry.glass.min.x + 10.0, geometry.glass.min.y + 3.0),
                Pos2::new(geometry.glass.max.x - 10.0, geometry.glass.min.y + 3.0),
            ],
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 34)),
        );
    }

    fn virtual_buttons(&mut self, ui: &mut egui::Ui) {
        let screen = self.menu.render();
        let width = ui.available_width().min(LITTLE_WIDTH);
        let (row, _) = ui.allocate_exact_size(Vec2::new(width, 66.0), Sense::hover());
        let mapped = row.shrink2(Vec2::new(12.0, 0.0));
        let column_width = mapped.width() / 4.0;
        for index in 0..4 {
            let center = Pos2::new(
                mapped.min.x + column_width * (index as f32 + 0.5),
                row.center().y,
            );
            let button_rect = Rect::from_center_size(center, Vec2::new(column_width - 20.0, 56.0));
            let label = &screen.footer[index].label;
            let response = ui.put(
                button_rect,
                egui::Button::new(RichText::new(label).size(17.0).strong())
                    .fill(Color32::from_rgb(47, 54, 59))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(94, 105, 111)))
                    .corner_radius(9.0),
            );
            let down = response.is_pointer_button_down_on();
            match (down, self.button_down[index]) {
                (true, None) => {
                    self.button_down[index] = Some(Instant::now());
                    self.menu.set_button_pressed(short_input(index), true);
                }
                (false, Some(started)) => {
                    self.button_down[index] = None;
                    self.menu.set_button_pressed(short_input(index), false);
                    self.apply_input(if started.elapsed() >= LONG_PRESS {
                        long_input(index)
                    } else {
                        short_input(index)
                    });
                }
                _ => {}
            }
            response.on_hover_text(format!(
                "Button {} · hold for long press · keyboard {}",
                index + 1,
                ["Q", "W", "E", "R"][index]
            ));
        }
    }

    fn button_is_down(&self, index: usize) -> bool {
        self.button_down[index].is_some() || self.keyboard_down[index].is_some()
    }
}

impl eframe::App for DesktopApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.keyboard(context);
        context.request_repaint_after(Duration::from_millis(16));
        egui::TopBottomPanel::top("desktop-toolbar").show(context, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("RACKFORGE").color(Color32::from_rgb(58, 216, 224)));
                ui.label(RichText::new("DESKTOP HOST · windows-x86-64").weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Open Web UI").clicked() {
                        match webbrowser::open(&self.web_url) {
                            Ok(()) => self.status = format!("Opened {}", self.web_url),
                            Err(error) => self.status = format!("Could not open browser: {error}"),
                        }
                    }
                    ui.monospace(&self.web_url);
                });
            });
        });
        egui::CentralPanel::default().show(context, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(28.0);
                self.little_display(ui);
                ui.add_space(24.0);
                self.virtual_buttons(ui);
                ui.add_space(18.0);
                ui.label(RichText::new(&self.status).weak());
            });
        });
    }
}

fn short_input(index: usize) -> Input {
    [
        Input::Button1,
        Input::Button2,
        Input::Button3,
        Input::Button4,
    ][index]
}

fn long_input(index: usize) -> Input {
    [
        Input::Button1Long,
        Input::Button2Long,
        Input::Button3Long,
        Input::Button4Long,
    ][index]
}

fn plugin_session_state(plugin: &DesktopPlugin) -> PluginInstanceState {
    PluginInstanceState {
        instance_id: InstanceId::new(plugin.instance_id.clone())
            .expect("desktop plugin instance id is validated during loading"),
        plugin_id: plugin.plugin_id.clone(),
        plugin_name: plugin.name.clone(),
        ui_layouts: vec!["little@1".into()],
        config_available: plugin.config_available,
        sounds: plugin
            .sounds
            .iter()
            .map(|sound| SoundSummary {
                id: sound.id.clone(),
                name: sound.name.clone(),
                bank: Some(sound.bank.clone()),
                detail: Some(sound.detail.clone()),
                editable: sound.editable,
            })
            .collect(),
        selected_sound_id: plugin.selected_sound_id.clone(),
    }
}

fn load_desktop_plugins(
    plugins_root: &Path,
    data_root: &Path,
) -> Result<(Vec<DesktopPlugin>, Vec<String>)> {
    fs::create_dir_all(plugins_root)
        .with_context(|| format!("creating plugin directory {}", plugins_root.display()))?;
    fs::create_dir_all(data_root)
        .with_context(|| format!("creating plugin data directory {}", data_root.display()))?;

    let mut package_roots = fs::read_dir(plugins_root)
        .with_context(|| format!("reading plugin directory {}", plugins_root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .filter(|root| root.join("rackforge-plugin.toml").is_file())
        .collect::<Vec<_>>();
    package_roots.sort();

    let mut plugins = Vec::new();
    let mut warnings = Vec::new();
    let mut ids = BTreeSet::new();
    for root in package_roots {
        match load_desktop_plugin(&root, data_root) {
            Ok(plugin) if ids.insert(plugin.plugin_id.clone()) => plugins.push(plugin),
            Ok(plugin) => warnings.push(format!("Duplicate plugin {} ignored", plugin.plugin_id)),
            Err(error) => warnings.push(format!("{}: {error:#}", root.display())),
        }
    }
    Ok((plugins, warnings))
}

fn load_desktop_plugin(root: &Path, data_root: &Path) -> Result<DesktopPlugin> {
    let package = PluginPackage::open(root)?;
    if package.manifest().kind != PluginKind::Instrument {
        bail!(
            "Desktop PLAY currently accepts instrument plugins, found {:?}",
            package.manifest().kind
        );
    }
    let instance_id = format!("desktop.{}", package.manifest().id);
    InstanceId::new(instance_id.clone()).map_err(anyhow::Error::msg)?;

    // SAFETY: Desktop only scans the user's installed RackForge plugin root.
    // Native packages are trusted by the same boundary as the appliance host.
    let loaded = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), Some(data_root)) }?;
    // Native plugin libraries are process-lifetime objects. Leaking this box is
    // intentional: unloading while an instance may hold ABI pointers is unsafe.
    let loaded: &'static LoadedPlugin = Box::leak(Box::new(loaded));
    let mut instance = loaded.create_instance()?;
    let catalog = instance.preset_catalog()?;
    let banks = catalog
        .banks
        .iter()
        .map(|bank| (bank.id.as_str(), bank.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let sounds = catalog
        .presets
        .iter()
        .map(|preset| {
            let bank = preset
                .bank
                .as_deref()
                .and_then(|id| banks.get(id).copied())
                .unwrap_or("Factory");
            let detail = preset
                .category
                .as_deref()
                .or(preset.description.as_deref())
                .unwrap_or("Preset");
            PlaySound::new(&preset.id, &preset.name, bank, detail).editable(preset.editable)
        })
        .collect::<Vec<_>>();
    let selected_sound_id = sounds.first().map(|sound| sound.id.clone());
    if let Some(id) = selected_sound_id.as_deref() {
        instance
            .load_preset(id)
            .with_context(|| format!("loading initial preset {id:?}"))?;
    }

    Ok(DesktopPlugin {
        instance_id,
        plugin_id: package.manifest().id.clone(),
        name: package.manifest().name.clone(),
        config_available: package.manifest().config_mode,
        sounds,
        selected_sound_id,
        instance,
    })
}

fn default_desktop_root() -> Result<PathBuf> {
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(local).join("RackForge"));
    }
    Ok(env::current_dir()?.join(".rackforge"))
}

fn parse_options() -> Result<Options> {
    let desktop_root = default_desktop_root()?;
    let mut options = Options {
        port: 8787,
        lan: false,
        plugins_root: desktop_root.join("plugins"),
        data_root: desktop_root.join("data"),
    };
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--port" => {
                options.port = arguments
                    .next()
                    .context("--port requires a value")?
                    .parse()
                    .context("invalid --port")?;
                if options.port < 1024 {
                    bail!("--port must be in 1024..=65535");
                }
            }
            "--lan" => options.lan = true,
            "--plugins-root" => {
                options.plugins_root = PathBuf::from(
                    arguments
                        .next()
                        .context("--plugins-root requires a directory")?,
                );
            }
            "--data-root" => {
                options.data_root = PathBuf::from(
                    arguments
                        .next()
                        .context("--data-root requires a directory")?,
                );
            }
            _ => bail!("unknown argument: {argument}"),
        }
    }
    Ok(options)
}

fn main() {
    if let Err(error) = run() {
        show_startup_error(&format!("{error:#}"));
    }
}

fn run() -> Result<()> {
    let options = parse_options()?;
    let session = Arc::new(RwLock::new(SessionState::new(
        SessionId::new(DEFAULT_LIVE_SESSION_ID).expect("valid live session id"),
    )));
    let app = DesktopApp::new(Arc::clone(&session), &options)?;
    web::start(Arc::clone(&session), options.port, options.lan)?;
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RackForge Desktop")
            .with_inner_size([920.0, 620.0])
            .with_min_inner_size([720.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RackForge Desktop",
        native,
        Box::new(move |_creation| Ok(Box::new(app))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}

#[cfg(windows)]
fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    let title = "RackForge could not start"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let message = message
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both pointers reference NUL-terminated UTF-16 buffers for the
    // duration of the synchronous MessageBoxW call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_startup_error(message: &str) {
    eprintln!("RackForge could not start: {message}");
}
