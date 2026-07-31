mod controller;
mod framebuffer;

use midir::{
    Ignore, MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection,
    MidiOutputPort,
};
#[cfg(target_os = "linux")]
use rackforge_control_api::{
    CONTROL_SOCKET_NAME, ControlRequest, ControlResponse, MAX_CONTROL_MESSAGE_BYTES,
    decode_response, encode_line,
};
#[cfg(target_os = "linux")]
use rackforge_controller_api::LITTLE_V1;
use rackforge_controller_package::{
    CONTROLLER_DRIVER_API_VERSION, PROCESS_DRIVER_PROTOCOL_VERSION, ProcessDriverInfo,
};
#[cfg(target_os = "linux")]
use rackforge_platform_api::{
    PLATFORM_CONTROL_SCHEMA_VERSION, PlatformControlPayload, PlatformControlRequest,
    PlatformControlResponse, PlatformOperation, WifiConnectionId, WifiPassphrase, WifiSsid,
};
#[cfg(target_os = "linux")]
use rackforge_session_api::{
    ClientId, CommandEnvelope, EventEnvelope, InstanceId, PluginInstanceState, SessionCommand,
    SessionEvent, SessionState, SurfaceActivationRequest, SurfaceMode,
};
use rackforge_session_api::{HostControlBinding, HostControlTarget, MasterLevel, MasterPan};
use rackforge_surface_runtime as menu;
use serde_json::Value;
use std::env;
use std::error::Error;
use std::fmt::Write as _;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::net::{Ipv4Addr, Shutdown};
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixStream;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

const PREFIX: &[u8] = &[0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42];
const CONNECT: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x02, 0x0F, 0x40, 0x5A, 0x01, 0xF7,
];
const DISCONNECT: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x02, 0x0F, 0x40, 0x5A, 0x00, 0xF7,
];
const CLEAR_SCREEN: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x60, 0x61, 0x00, 0xF7,
];
const USB_BOOT_STABILITY: Duration = Duration::from_secs(5);
const ACQUIRE_RETRY_DELAY: Duration = Duration::from_secs(2);
const LONG_PRESS_THRESHOLD: Duration = Duration::from_millis(650);
const HOME_CHORD_SIMULTANEITY: Duration = Duration::from_millis(250);
const HOST_CONTROL_HEADER_TIMEOUT: Duration = Duration::from_millis(1_500);
const SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(125);
#[cfg(target_os = "linux")]
const WEB_CONTROL_SOCKET_NAME: &str = "web-control.sock";
#[cfg(target_os = "linux")]
const PLATFORM_CONTROL_SOCKET: &str = "/run/rackforge/platform-control.sock";
#[cfg(target_os = "linux")]
static NEXT_CONTROL_COMMAND_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(target_os = "linux")]
#[derive(Debug)]
enum WifiTaskSuccess {
    Scan {
        networks: Vec<menu::DiscoveredWifiNetwork>,
        settings: menu::WifiSystemSettings,
    },
    Changed {
        message: &'static str,
        settings: menu::WifiSystemSettings,
    },
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct WifiTask {
    receiver: Receiver<Result<WifiTaskSuccess, String>>,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct AudioTask {
    receiver: Receiver<Result<rackforge_control_api::AudioOutputState, String>>,
}

#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
struct WifiTask;

#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
struct AudioTask;

#[derive(Debug)]
struct Cli {
    command: Command,
}

#[derive(Debug)]
enum Command {
    DriverInfo,
    SelfTest,
    List,
    Demo {
        selector: Option<String>,
        seconds: u64,
        execute: bool,
    },
    MenuDemo {
        selector: Option<String>,
        seconds: u64,
        execute: bool,
    },
    FramebufferDemo {
        selector: Option<String>,
        seconds: u64,
        execute: bool,
    },
    Serve {
        selector: Option<String>,
        execute: bool,
    },
    Restore {
        selector: Option<String>,
        execute: bool,
    },
    LedDemo {
        selector: Option<String>,
        execute: bool,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = parse_args(env::args().skip(1))?;
    match &cli.command {
        Command::DriverInfo => {
            let profile = controller::package_profile();
            println!(
                "{}",
                serde_json::to_string(&ProcessDriverInfo {
                    protocol_version: PROCESS_DRIVER_PROTOCOL_VERSION,
                    id: profile.driver_id.clone(),
                    controller_api: CONTROLLER_DRIVER_API_VERSION.into(),
                    layouts: profile
                        .surfaces
                        .iter()
                        .map(|surface| surface.layout_id.clone())
                        .collect(),
                    host_controls: profile.host_controls.clone(),
                })?
            );
            return Ok(());
        }
        Command::SelfTest => {
            run_driver_self_test()?;
            return Ok(());
        }
        _ => {}
    }
    let midi = MidiOutput::new("rackforge KeyLab Bridge")?;
    let ports = enumerate_ports(&midi)?;

    match cli.command {
        Command::DriverInfo | Command::SelfTest => unreachable!("handled before opening MIDI"),
        Command::List => print_ports(&ports),
        Command::Demo {
            selector,
            seconds,
            execute,
        } => {
            let selected = select_port(&ports, selector.as_deref())?;
            run_demo(midi, selected, seconds, execute)?;
        }
        Command::MenuDemo {
            selector,
            seconds,
            execute,
        } => {
            let selected = select_port(&ports, selector.as_deref())?;
            run_menu_demo(midi, selected, seconds, execute)?;
        }
        Command::FramebufferDemo {
            selector,
            seconds,
            execute,
        } => {
            let selected = select_port(&ports, selector.as_deref())?;
            run_framebuffer_demo(midi, selected, seconds, execute)?;
        }
        Command::Serve { selector, execute } => {
            run_serve(selector.as_deref(), execute)?;
        }
        Command::Restore { selector, execute } => {
            let selected = select_port(&ports, selector.as_deref())?;
            run_restore(midi, selected, execute)?;
        }
        Command::LedDemo { selector, execute } => {
            let selected = select_port(&ports, selector.as_deref())?;
            run_led_demo(midi, selected, execute)?;
        }
    }
    Ok(())
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Cli, String> {
    let args: Vec<String> = args.collect();
    if args.is_empty() || args == ["list"] {
        return Ok(Cli {
            command: Command::List,
        });
    }
    let command = args.first().map(String::as_str);
    if args == ["driver-info"] {
        return Ok(Cli {
            command: Command::DriverInfo,
        });
    }
    if args == ["self-test"] {
        return Ok(Cli {
            command: Command::SelfTest,
        });
    }
    if !matches!(
        command,
        Some("demo" | "menu-demo" | "framebuffer-demo" | "serve" | "restore" | "led-demo")
    ) {
        return Err(usage("Comando desconocido"));
    }

    let mut selector = None;
    let mut seconds = 30_u64;
    let mut execute = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--port" => {
                index += 1;
                selector = Some(
                    args.get(index)
                        .ok_or_else(|| usage("--port requiere un valor"))?
                        .clone(),
                );
            }
            "--seconds" if matches!(command, Some("demo" | "menu-demo" | "framebuffer-demo")) => {
                index += 1;
                seconds = args
                    .get(index)
                    .ok_or_else(|| usage("--seconds requiere un valor"))?
                    .parse()
                    .map_err(|_| usage("--seconds debe ser un entero"))?;
            }
            "--execute" => execute = true,
            flag => return Err(usage(&format!("Opción desconocida: {flag}"))),
        }
        index += 1;
    }
    if !(1..=120).contains(&seconds) {
        return Err(usage("--seconds debe estar entre 1 y 120"));
    }
    let command = match command {
        Some("menu-demo") => Command::MenuDemo {
            selector,
            seconds,
            execute,
        },
        Some("framebuffer-demo") => Command::FramebufferDemo {
            selector,
            seconds,
            execute,
        },
        Some("serve") => Command::Serve { selector, execute },
        Some("restore") => Command::Restore { selector, execute },
        Some("led-demo") => Command::LedDemo { selector, execute },
        _ => Command::Demo {
            selector,
            seconds,
            execute,
        },
    };
    Ok(Cli { command })
}

fn usage(reason: &str) -> String {
    format!(
        "{reason}\n\
         Uso:\n\
           rackforge-arturia-keylab-essential-mk3-driver driver-info\n\
           rackforge-arturia-keylab-essential-mk3-driver self-test\n\
           rackforge-arturia-keylab-essential-mk3-driver list\n\
           rackforge-arturia-keylab-essential-mk3-driver demo [--port ID|NOMBRE] [--seconds 1..120] [--execute]\n\
           rackforge-arturia-keylab-essential-mk3-driver menu-demo [--port ID|NOMBRE] [--seconds 1..120] [--execute]\n\
           rackforge-arturia-keylab-essential-mk3-driver framebuffer-demo [--port ID|NOMBRE] [--seconds 1..120] [--execute]\n\
           rackforge-arturia-keylab-essential-mk3-driver serve [--port ID|NOMBRE] [--execute]\n\
           rackforge-arturia-keylab-essential-mk3-driver restore [--port ID|NOMBRE] [--execute]\n\
           rackforge-arturia-keylab-essential-mk3-driver led-demo [--port ID|NOMBRE] [--execute]"
    )
}

fn run_driver_self_test() -> Result<(), Box<dyn Error>> {
    controller::package_profile().validate()?;
    let daw = select_preset(1)?;
    let arturia = select_preset(0)?;
    let title = header("RACKFORGE")?;
    let body = two_lines("DRIVER SELF TEST", "PROTOCOL OK")?;
    let fixtures: Value = serde_json::from_str(include_str!("../fixtures/protocol-v1.json"))?;
    let expected_daw = fixtures
        .get("daw_preset_ack")
        .and_then(Value::as_str)
        .ok_or("fixture daw_preset_ack is missing")?;
    let expected_arturia = fixtures
        .get("arturia_preset")
        .and_then(Value::as_str)
        .ok_or("fixture arturia_preset is missing")?;
    let physical_messages = fixtures
        .get("physical_input_messages")
        .and_then(Value::as_array)
        .ok_or("fixture physical_input_messages is missing")?;
    let inputs_are_known = physical_messages.iter().all(|fixture| {
        fixture
            .as_array()
            .and_then(|bytes| {
                bytes
                    .iter()
                    .map(|byte| byte.as_u64().and_then(|byte| u8::try_from(byte).ok()))
                    .collect::<Option<Vec<_>>>()
            })
            .is_some_and(|message| parse_physical_input(&message).is_some())
    });
    if hex(&daw) != expected_daw
        || hex(&arturia) != expected_arturia
        || !is_daw_preset_ack(&daw)
        || is_daw_preset_ack(&arturia)
        || !title.starts_with(PREFIX)
        || !body.starts_with(PREFIX)
        || !inputs_are_known
    {
        return Err("falló la conformidad interna del protocolo KeyLab".into());
    }
    println!(
        "CONTROLLER_SELF_TEST_OK id={}",
        controller::package_profile().driver_id
    );
    Ok(())
}

#[derive(Clone)]
struct PortInfo {
    index: usize,
    name: String,
    handle: MidiOutputPort,
}

#[derive(Clone)]
struct InputPortInfo {
    index: usize,
    name: String,
    handle: MidiInputPort,
}

struct KeyLabInput {
    _connection: MidiInputConnection<()>,
    ack_receiver: Receiver<()>,
    input_receiver: Receiver<PhysicalInputEvent>,
    host_control_receiver: Receiver<HostControlEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputPhase {
    Press,
    Release,
    Turn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PhysicalInputEvent {
    input: menu::Input,
    phase: InputPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HostControlEvent {
    target: HostControlTarget,
    value: u8,
}

impl HostControlEvent {
    fn header_text(self) -> String {
        match self.target {
            HostControlTarget::MasterLevel => {
                let level = MasterLevel::from_midi(self.value);
                let percent = (u32::from(level.get()) + 5) / 10;
                format!("MASTER VOL {:>7}", format!("{percent}%"))
            }
            HostControlTarget::MasterPan => {
                let pan = MasterPan::from_midi_with_center_snap(self.value);
                let value = pan.get();
                let label = if value == 0 {
                    "CENTER".to_owned()
                } else {
                    let side = if value < 0 { 'L' } else { 'R' };
                    let percent = (u32::from(value.unsigned_abs()) + 5) / 10;
                    format!("{side} {percent}%")
                };
                format!("MASTER PAN {:>7}", label)
            }
        }
    }
}

#[derive(Debug)]
struct ActiveTransientHeader {
    message: Vec<u8>,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct TransientHeader {
    active: Option<ActiveTransientHeader>,
}

impl TransientHeader {
    fn show(&mut self, text: &str, now: Instant) -> Result<&[u8], String> {
        self.active = Some(ActiveTransientHeader {
            message: header(text)?,
            expires_at: now + HOST_CONTROL_HEADER_TIMEOUT,
        });
        Ok(&self.active.as_ref().expect("header was just set").message)
    }

    fn visible_message(&self, now: Instant) -> Option<&[u8]> {
        self.active
            .as_ref()
            .filter(|active| now < active.expires_at)
            .map(|active| active.message.as_slice())
    }

    fn expire(&mut self, now: Instant) -> bool {
        if self
            .active
            .as_ref()
            .is_some_and(|active| now >= active.expires_at)
        {
            self.active = None;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct HeldButton {
    pressed_at: Instant,
    long_emitted: bool,
}

#[derive(Debug, Default)]
struct ButtonGestureTracker {
    held: [Option<HeldButton>; 4],
    home_chord_emitted: bool,
}

impl ButtonGestureTracker {
    fn press(&mut self, input: menu::Input, now: Instant) -> bool {
        let Some(index) = soft_button_index(input) else {
            return false;
        };
        self.held[index].get_or_insert(HeldButton {
            pressed_at: now,
            long_emitted: false,
        });
        true
    }

    fn release(&mut self, input: menu::Input, now: Instant) -> Option<menu::Input> {
        let index = soft_button_index(input)?;
        let held = self.held[index].take()?;
        let gesture = if held.long_emitted {
            None
        } else if now.saturating_duration_since(held.pressed_at) >= LONG_PRESS_THRESHOLD {
            input.long_press()
        } else {
            Some(input)
        };
        if self.held.iter().all(Option::is_none) {
            self.home_chord_emitted = false;
        }
        gesture
    }

    fn poll(&mut self, now: Instant) -> Vec<menu::Input> {
        let mut gestures = Vec::new();
        if !self.home_chord_emitted
            && let (Some(ok), Some(back)) = (self.held[0], self.held[3])
        {
            let separation = if ok.pressed_at >= back.pressed_at {
                ok.pressed_at.duration_since(back.pressed_at)
            } else {
                back.pressed_at.duration_since(ok.pressed_at)
            };
            let chord_started = ok.pressed_at.max(back.pressed_at);
            if separation <= HOME_CHORD_SIMULTANEITY
                && now.saturating_duration_since(chord_started) >= LONG_PRESS_THRESHOLD
            {
                self.home_chord_emitted = true;
                self.held[0].as_mut().expect("OK is held").long_emitted = true;
                self.held[3].as_mut().expect("BACK is held").long_emitted = true;
                gestures.push(menu::Input::HomeChord);
            }
        }
        for (index, held) in self.held.iter_mut().enumerate() {
            let Some(held) = held else {
                continue;
            };
            if !held.long_emitted
                && now.saturating_duration_since(held.pressed_at) >= LONG_PRESS_THRESHOLD
            {
                held.long_emitted = true;
                let input = [
                    menu::Input::Button1,
                    menu::Input::Button2,
                    menu::Input::Button3,
                    menu::Input::Button4,
                ][index];
                gestures.push(input.long_press().expect("soft buttons have long gestures"));
            }
        }
        gestures
    }
}

fn soft_button_index(input: menu::Input) -> Option<usize> {
    match input {
        menu::Input::Button1 => Some(0),
        menu::Input::Button2 => Some(1),
        menu::Input::Button3 => Some(2),
        menu::Input::Button4 => Some(3),
        _ => None,
    }
}

fn enumerate_ports(midi: &MidiOutput) -> Result<Vec<PortInfo>, Box<dyn Error>> {
    midi.ports()
        .into_iter()
        .enumerate()
        .map(|(index, handle)| {
            Ok(PortInfo {
                index,
                name: midi.port_name(&handle)?,
                handle,
            })
        })
        .collect()
}

fn enumerate_input_ports(midi: &MidiInput) -> Result<Vec<InputPortInfo>, Box<dyn Error>> {
    midi.ports()
        .into_iter()
        .enumerate()
        .map(|(index, handle)| {
            Ok(InputPortInfo {
                index,
                name: midi.port_name(&handle)?,
                handle,
            })
        })
        .collect()
}

fn is_keylab_midi(name: &str) -> bool {
    controller::display_driver(name).is_some()
}

fn print_ports(ports: &[PortInfo]) {
    println!("Puertos MIDI de salida:");
    for port in ports {
        let marker = controller::display_driver(&port.name)
            .map(|driver| format!("  <{} / {}>", driver.profile().name, "little@1"))
            .unwrap_or_default();
        println!("  [{}] {}{}", port.index, port.name, marker);
    }
}

fn select_port<'a>(ports: &'a [PortInfo], selector: Option<&str>) -> Result<&'a PortInfo, String> {
    let matches: Vec<&PortInfo> = if let Some(selector) = selector {
        if let Ok(index) = selector.parse::<usize>() {
            ports.iter().filter(|port| port.index == index).collect()
        } else {
            let needle = selector.to_ascii_lowercase();
            ports
                .iter()
                .filter(|port| port.name.to_ascii_lowercase().contains(&needle))
                .collect()
        }
    } else {
        ports
            .iter()
            .filter(|port| controller::display_driver(&port.name).is_some())
            .collect()
    };

    match matches.as_slice() {
        [] => Err("No se encontró el puerto MIDI del KeyLab".into()),
        [port] => Ok(*port),
        many => {
            let options = many
                .iter()
                .map(|port| format!("[{}] {}", port.index, port.name))
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!("Selección ambigua: {options}"))
        }
    }
}

fn select_input_port<'a>(
    ports: &'a [InputPortInfo],
    selector: Option<&str>,
) -> Result<&'a InputPortInfo, String> {
    let matches: Vec<&InputPortInfo> = if let Some(selector) = selector {
        if let Ok(index) = selector.parse::<usize>() {
            ports.iter().filter(|port| port.index == index).collect()
        } else {
            let needle = selector.to_ascii_lowercase();
            ports
                .iter()
                .filter(|port| port.name.to_ascii_lowercase().contains(&needle))
                .collect()
        }
    } else {
        ports
            .iter()
            .filter(|port| controller::surface_input_driver(&port.name).is_some())
            .collect()
    };

    match matches.as_slice() {
        [] => Err("No se encontró la entrada MIDI del KeyLab".into()),
        [port] => Ok(*port),
        many => {
            let options = many
                .iter()
                .map(|port| format!("[{}] {}", port.index, port.name))
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!("Selección de entrada ambigua: {options}"))
        }
    }
}

fn sysex(payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.iter().any(|byte| *byte > 0x7F) {
        return Err("El payload contiene datos fuera del rango MIDI de 7 bits".into());
    }
    let mut message = Vec::with_capacity(PREFIX.len() + payload.len() + 1);
    message.extend_from_slice(PREFIX);
    message.extend_from_slice(payload);
    message.push(0xF7);
    Ok(message)
}

fn select_preset(index: u8) -> Result<Vec<u8>, String> {
    if index > 7 {
        return Err("El índice de programa debe estar entre 0 y 7".into());
    }
    sysex(&[0x21, 0x11, 0x40, 0x02, 0x00, index])
}

fn ascii_text(text: &str) -> Result<&[u8], String> {
    if text.is_empty() || text.len() > 18 {
        return Err("El texto debe contener entre 1 y 18 caracteres".into());
    }
    if !text.is_ascii() || text.as_bytes().contains(&0) {
        return Err("La pantalla solo admite ASCII sin NUL".into());
    }
    Ok(text.as_bytes())
}

fn header(text: &str) -> Result<Vec<u8>, String> {
    let mut payload = vec![0x04, 0x01, 0x60, 0x01, 0x02];
    payload.extend_from_slice(ascii_text(text)?);
    payload.extend_from_slice(&[0x00, 0x00]);
    sysex(&payload)
}

fn two_lines(line_1: &str, line_2: &str) -> Result<Vec<u8>, String> {
    let mut payload = vec![0x04, 0x01, 0x60, 0x12, 0x01];
    payload.extend_from_slice(ascii_text(line_1)?);
    payload.extend_from_slice(&[0x00, 0x02]);
    payload.extend_from_slice(ascii_text(line_2)?);
    payload.extend_from_slice(&[0x00, 0x00]);
    sysex(&payload)
}

fn footer(buttons: &[menu::FooterButton; 4]) -> Result<Vec<u8>, String> {
    let mut payload = vec![0x04, 0x01, 0x60, 0x03];
    for (index, button) in buttons.iter().enumerate() {
        let label = &button.label;
        if label.is_empty() || label.len() > 7 || !label.is_ascii() || label.as_bytes().contains(&0)
        {
            return Err(
                "Cada etiqueta del footer debe contener entre 1 y 7 caracteres ASCII sin NUL"
                    .into(),
            );
        }
        let frame = match button.state {
            rackforge_ui::VisualState::Normal => 0x00,
            rackforge_ui::VisualState::Focused => 0x02,
            rackforge_ui::VisualState::Pressed => 0x03,
            rackforge_ui::VisualState::Disabled => 0x00,
        };
        payload.push(0x10 + (index as u8 * 0x10));
        payload.extend_from_slice(&[frame, 0x00]);
        payload.push(0x11 + (index as u8 * 0x10));
        payload.extend_from_slice(label.as_bytes());
        payload.push(0x00);
    }
    sysex(&payload)
}

struct KeyLabSession {
    connection: MidiOutputConnection,
    switched_to_daw: bool,
    connected: bool,
}

impl KeyLabSession {
    fn open(midi: MidiOutput, port: &PortInfo) -> Result<Self, Box<dyn Error>> {
        let Some(driver) = controller::little_driver(&port.name) else {
            return Err(format!(
                "El puerto [{}] {} no tiene un driver LITTLE certificado; no se enviará SysEx",
                port.index, port.name
            )
            .into());
        };
        driver.profile().validate()?;
        let connection = midi.connect(&port.handle, "rackforge KeyLab SysEx")?;
        Ok(Self {
            connection,
            switched_to_daw: false,
            connected: false,
        })
    }

    fn send(&mut self, message: &[u8]) -> Result<(), Box<dyn Error>> {
        self.connection.send(message)?;
        Ok(())
    }

    fn start(&mut self) -> Result<(), Box<dyn Error>> {
        self.send(&select_preset(1)?)?;
        self.switched_to_daw = true;
        thread::sleep(Duration::from_millis(350));
        self.send(CONNECT)?;
        self.connected = true;
        thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    fn restore(&mut self) -> Result<(), Box<dyn Error>> {
        let mut failures = Vec::new();
        if self.connected {
            if let Err(error) = self.send(CLEAR_SCREEN) {
                failures.push(format!("limpiar pantalla: {error}"));
            }
            if let Err(error) = self.send(DISCONNECT) {
                failures.push(format!("desconectar DAW: {error}"));
            }
            self.connected = false;
            thread::sleep(Duration::from_millis(150));
        }
        if self.switched_to_daw {
            match select_preset(0) {
                Ok(message) => {
                    if let Err(error) = self.send(&message) {
                        failures.push(format!("restaurar Arturia: {error}"));
                    }
                }
                Err(error) => failures.push(error),
            }
            self.switched_to_daw = false;
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

impl Drop for KeyLabSession {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("ADVERTENCIA: restauración incompleta: {error}");
        }
    }
}

fn hex(message: &[u8]) -> String {
    let mut output = String::with_capacity(message.len() * 3);
    for (index, byte) in message.iter().enumerate() {
        if index > 0 {
            output.push(' ');
        }
        write!(&mut output, "{byte:02X}").expect("writing to String cannot fail");
    }
    output
}

fn run_demo(
    midi: MidiOutput,
    port: &PortInfo,
    seconds: u64,
    execute: bool,
) -> Result<(), Box<dyn Error>> {
    let daw = select_preset(1)?;
    let title = header("RACKFORGE")?;
    let screen = two_lines("RACKFORGE", "PI CONNECTED")?;
    let arturia = select_preset(0)?;

    println!("Puerto: [{}] {}", port.index, port.name);
    if !execute {
        println!("DRY-RUN: no se enviará nada.");
        for (label, message) in [
            ("preset-daw", daw.as_slice()),
            ("connect", CONNECT),
            ("header", title.as_slice()),
            ("screen", screen.as_slice()),
            ("clear", CLEAR_SCREEN),
            ("disconnect", DISCONNECT),
            ("preset-arturia", arturia.as_slice()),
        ] {
            println!("{label:15} {}", hex(message));
        }
        return Ok(());
    }

    let mut session = KeyLabSession::open(midi, port)?;
    session.start()?;
    session.send(&title)?;
    thread::sleep(Duration::from_millis(50));
    session.send(&screen)?;
    println!("Demo Rust activa durante {seconds} segundos...");
    thread::sleep(Duration::from_secs(seconds));
    session.restore()?;
    println!("Pantalla y programa Arturia restaurados.");
    Ok(())
}

fn run_menu_demo(
    midi: MidiOutput,
    port: &PortInfo,
    seconds: u64,
    execute: bool,
) -> Result<(), Box<dyn Error>> {
    let screens = menu::demo_frames()
        .into_iter()
        .map(|screen| {
            let header_message = screen_header_message(&screen.header)?;
            let body_message = two_lines(&screen.line_1, &screen.line_2)?;
            Ok((screen, header_message, body_message))
        })
        .collect::<Result<Vec<_>, String>>()?;
    println!("Puerto: [{}] {}", port.index, port.name);
    if !execute {
        println!("DRY-RUN: no se enviará nada.");
        for (index, (screen, header_message, body_message)) in screens.iter().enumerate() {
            println!(
                "menu-{} {:?} / {:?} / {:?}: {} | {}",
                index + 1,
                screen.header,
                screen.line_1,
                screen.line_2,
                hex(header_message),
                hex(body_message)
            );
        }
        return Ok(());
    }

    let mut session = KeyLabSession::open(midi, port)?;
    session.start()?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let frame_time = Duration::from_secs(3);
    let mut index = 0;
    while Instant::now() < deadline {
        let (_, header_message, body_message) = &screens[index % screens.len()];
        session.send(header_message)?;
        thread::sleep(Duration::from_millis(20));
        session.send(body_message)?;
        index += 1;
        thread::sleep(frame_time.min(deadline.saturating_duration_since(Instant::now())));
    }
    session.restore()?;
    println!("Demo de menú finalizada; pantalla Arturia restaurada.");
    Ok(())
}

fn run_framebuffer_demo(
    midi: MidiOutput,
    port: &PortInfo,
    seconds: u64,
    execute: bool,
) -> Result<(), Box<dyn Error>> {
    let messages = framebuffer::solid_upload(true);
    println!(
        "Puerto: [{}] {}\nFrame completo: {} mensajes acotados",
        port.index,
        port.name,
        messages.len()
    );
    if !execute {
        println!("DRY-RUN: no se enviará el framebuffer.");
        return Ok(());
    }

    let mut session = KeyLabSession::open(midi, port)?;
    session.start()?;
    for message in &messages {
        session.send(message)?;
        thread::sleep(Duration::from_millis(2));
    }
    println!("Framebuffer sólido activo durante {seconds} segundos...");
    thread::sleep(Duration::from_secs(seconds));
    session.restore()?;
    println!("Prueba de framebuffer finalizada; programa Arturia restaurado.");
    Ok(())
}

fn run_serve(selector: Option<&str>, execute: bool) -> Result<(), Box<dyn Error>> {
    if !execute {
        println!("DRY-RUN: el servicio esperaría al KeyLab y mantendría HOME en la OLED.");
        println!("Agregá --execute para iniciar la sesión persistente.");
        return Ok(());
    }

    let mut menu = menu::Menu::default();
    if let Err(error) = refresh_live_catalog(&mut menu) {
        eprintln!("Catálogo LIVE todavía no disponible: {error}");
    }
    println!("Esperando el KeyLab Essential mk3...");
    loop {
        let midi = MidiOutput::new("rackforge KeyLab Display")?;
        let ports = enumerate_ports(&midi)?;
        let port = match select_port(&ports, selector) {
            Ok(port) => port,
            Err(_) => {
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let usb_generation = keylab_usb_generation();
        if let Err(error) = register_host_controls() {
            eprintln!("Core todavía no acepta el perfil del controlador: {error}");
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        let control_generation = control_socket_generation();
        if let Some(generation) = usb_generation.as_deref() {
            println!("KeyLab USB detectado ({generation}); esperando arranque estable...");
            if !wait_for_keylab_usb(generation, USB_BOOT_STABILITY) {
                eprintln!("El KeyLab cambió durante el arranque; reintentando...");
                continue;
            }
        }
        if control_socket_generation() != control_generation {
            eprintln!("Core cambió durante la espera USB; renovando el perfil...");
            continue;
        }
        let input = match open_keylab_input(selector) {
            Ok(channel) => channel,
            Err(error) => {
                eprintln!("La entrada de confirmación todavía no está disponible: {error}");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        menu.clear_pressed_button();
        let mut messages = render_menu_messages(&menu)?;
        let port_name = port.name.clone();
        let mut session = match KeyLabSession::open(midi, port) {
            Ok(session) => session,
            Err(error) => {
                eprintln!("KeyLab todavía no disponible: {error}");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        match acquire_screen(
            &mut session,
            &messages,
            &input.ack_receiver,
            usb_generation.as_deref(),
        ) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("El KeyLab cambió durante la adquisición; reintentando...");
                continue;
            }
            Err(error) => {
                eprintln!("No se pudo tomar la pantalla: {error}");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        }

        println!("OLED bajo control de RackForge: {port_name}");
        let mut next_heartbeat = Instant::now() + Duration::from_secs(6);
        let mut missed_acks = 0_u8;
        let mut button_gestures = ButtonGestureTracker::default();
        let mut transient_header = TransientHeader::default();
        let mut wifi_task: Option<WifiTask> = None;
        let mut audio_task: Option<AudioTask> = None;
        let mut next_spinner_frame = Instant::now();
        'surface: loop {
            if control_socket_generation() != control_generation {
                eprintln!("Core cambió; cerrando MIDI para renovar los controles reservados...");
                break;
            }
            if usb_generation.is_some() && keylab_usb_generation() != usb_generation {
                eprintln!("Cambió la instancia USB del KeyLab; creando una sesión MIDI nueva...");
                break;
            }

            let host_control_events =
                coalesce_host_control_events(input.host_control_receiver.try_iter());
            let mut latest_feedback = None;
            for event in host_control_events {
                match apply_host_control(event) {
                    Ok(()) => {
                        latest_feedback = Some(event.header_text());
                        println!("HOST_CONTROL {event:?}");
                    }
                    Err(error) => {
                        eprintln!("No se pudo aplicar el control reservado del host: {error}")
                    }
                }
            }
            if let Some(feedback) = latest_feedback {
                match transient_header.show(&feedback, Instant::now()) {
                    Ok(message) => {
                        if let Err(error) = session.send(message) {
                            eprintln!("No se pudo mostrar el control maestro: {error}");
                            break 'surface;
                        }
                    }
                    Err(error) => {
                        eprintln!("No se pudo componer el control maestro: {error}");
                    }
                }
            }

            match input.input_receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(event) => {
                    let mut navigation_input = None;
                    match event.phase {
                        InputPhase::Press => {
                            if button_gestures.press(event.input, Instant::now()) {
                                menu.set_button_pressed(event.input, true);
                                messages = render_menu_messages(&menu)?;
                                if let Err(error) = session.send(&messages.footer) {
                                    eprintln!("No se pudo mostrar el botón presionado: {error}");
                                    break;
                                }
                            } else {
                                navigation_input = Some(event.input);
                            }
                        }
                        InputPhase::Release => {
                            navigation_input = button_gestures.release(event.input, Instant::now());
                            if menu.set_button_pressed(event.input, false) {
                                messages = render_menu_messages(&menu)?;
                                if let Err(error) = session.send(&messages.footer) {
                                    eprintln!("No se pudo restaurar el footer: {error}");
                                    break;
                                }
                            }
                        }
                        InputPhase::Turn => {
                            navigation_input = Some(event.input);
                        }
                    }
                    if let Some(navigation_input) = navigation_input {
                        match send_menu_frames(
                            &mut session,
                            vec![menu.apply_input_and_render(navigation_input)],
                            transient_header.visible_message(Instant::now()),
                        ) {
                            Ok(rendered) => messages = rendered,
                            Err(error) => {
                                eprintln!("No se pudo actualizar el menú: {error}");
                                break;
                            }
                        }
                    }
                    match apply_pending_menu_command(&mut menu, &mut wifi_task, &mut audio_task) {
                        Ok(true) => {
                            messages = render_menu_messages(&menu)?;
                            if let Err(error) = send_menu_with_header_override(
                                &mut session,
                                &messages,
                                transient_header.visible_message(Instant::now()),
                            ) {
                                eprintln!("No se pudo confirmar la selección: {error}");
                                break;
                            }
                        }
                        Ok(false) => {}
                        Err(error) => eprintln!("No se pudo seleccionar el sonido: {error}"),
                    }
                    println!("INPUT {event:?}");
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    eprintln!("Se cerró el canal de controles; volviendo a adquisición...");
                    break;
                }
            }

            for long_input in button_gestures.poll(Instant::now()) {
                match send_menu_frames(
                    &mut session,
                    vec![menu.apply_input_and_render(long_input)],
                    transient_header.visible_message(Instant::now()),
                ) {
                    Ok(rendered) => messages = rendered,
                    Err(error) => {
                        eprintln!("No se pudo aplicar la pulsación prolongada: {error}");
                        break 'surface;
                    }
                }
                match apply_pending_menu_command(&mut menu, &mut wifi_task, &mut audio_task) {
                    Ok(true) => {
                        messages = render_menu_messages(&menu)?;
                        if let Err(error) = send_menu_with_header_override(
                            &mut session,
                            &messages,
                            transient_header.visible_message(Instant::now()),
                        ) {
                            eprintln!("No se pudo confirmar la pulsación prolongada: {error}");
                            break 'surface;
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!("No se pudo aplicar la pulsación prolongada: {error}")
                    }
                }
                println!("INPUT_LONG {long_input:?}");
            }

            let now = Instant::now();
            if poll_wifi_task(&mut wifi_task, &mut menu) {
                messages = render_menu_messages(&menu)?;
                if let Err(error) = send_menu_with_header_override(
                    &mut session,
                    &messages,
                    transient_header.visible_message(now),
                ) {
                    eprintln!("Could not show Wi-Fi operation result: {error}");
                    break;
                }
            } else if poll_audio_task(&mut audio_task, &mut menu) {
                messages = render_menu_messages(&menu)?;
                if let Err(error) = send_menu_with_header_override(
                    &mut session,
                    &messages,
                    transient_header.visible_message(now),
                ) {
                    eprintln!("Could not show audio operation result: {error}");
                    break;
                }
            } else if (wifi_task.is_some() || audio_task.is_some()) && now >= next_spinner_frame {
                next_spinner_frame = now + SPINNER_FRAME_INTERVAL;
                if menu.advance_wifi_spinner() || menu.advance_audio_spinner() {
                    messages = render_menu_messages(&menu)?;
                    if let Err(error) = session.send(&messages.body) {
                        eprintln!("Could not advance async loader: {error}");
                        break;
                    }
                }
            }
            if transient_header.expire(now) {
                if let Err(error) = session.send(&messages.header) {
                    eprintln!("No se pudo restaurar el header del menú: {error}");
                    break;
                }
            }

            if now < next_heartbeat {
                continue;
            }
            next_heartbeat = Instant::now() + Duration::from_secs(6);
            match verify_daw_ack(&mut session, &input.ack_receiver) {
                Ok(true) => {
                    missed_acks = 0;
                    println!("HEALTHY OLED_ACK");
                    if let Some(lease_id) = menu.audition_lease_id() {
                        match keep_audition_alive(lease_id) {
                            Ok(()) => {}
                            Err(message) => {
                                eprintln!("Se perdió el foco de audition: {message}");
                                menu.sync_program_edit(None, None);
                            }
                        }
                    }
                    if wifi_task.is_none() {
                        if let Err(error) = refresh_live_catalog(&mut menu) {
                            eprintln!("No se pudo refrescar el catálogo LIVE: {error}");
                        } else {
                            messages = render_menu_messages(&menu)?;
                        }
                    }
                    if let Err(error) = send_menu_with_header_override(
                        &mut session,
                        &messages,
                        transient_header.visible_message(Instant::now()),
                    ) {
                        eprintln!("No se pudo reafirmar el menú: {error}");
                        break;
                    }
                }
                Ok(false) => {
                    missed_acks += 1;
                    eprintln!("Heartbeat OLED sin ACK ({missed_acks}/2)");
                    if missed_acks >= 2 {
                        eprintln!("Sesión OLED no saludable; volviendo a adquisición...");
                        break;
                    }
                }
                Err(error) => {
                    eprintln!("Falló el heartbeat OLED: {error}");
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(target_os = "linux")]
fn control_socket_path() -> PathBuf {
    env::var_os("RACKFORGE_CONTROL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = env::var_os("RACKFORGE_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/home/kalex/rackforge"));
            root.join("state").join(CONTROL_SOCKET_NAME)
        })
}

#[cfg(target_os = "linux")]
fn control_socket_generation() -> Option<(u64, u64, i64, i64)> {
    fs::metadata(control_socket_path()).ok().map(|metadata| {
        (
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        )
    })
}

#[cfg(not(target_os = "linux"))]
fn control_socket_generation() -> Option<(u64, u64, i64, i64)> {
    None
}

#[cfg(target_os = "linux")]
fn control_request(request: &ControlRequest) -> Result<ControlResponse, String> {
    control_request_with_timeout(request, Duration::from_secs(1))
}

#[cfg(target_os = "linux")]
fn control_request_with_timeout(
    request: &ControlRequest,
    timeout: Duration,
) -> Result<ControlResponse, String> {
    let path = control_socket_path();
    let mut stream =
        UnixStream::connect(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| error.to_string())?;
    let bytes = encode_line(request).map_err(|error| error.to_string())?;
    stream
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .take((MAX_CONTROL_MESSAGE_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    if response.is_empty() || response.len() > MAX_CONTROL_MESSAGE_BYTES {
        return Err("respuesta de control vacía o demasiado grande".into());
    }
    decode_response(&response).map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn live_snapshot() -> Result<SessionState, String> {
    match control_request(&ControlRequest::Snapshot)? {
        ControlResponse::Snapshot { snapshot } => Ok(*snapshot),
        ControlResponse::Error { message, .. } => Err(message),
        _ => Err("respuesta inesperada al pedir estado de sesión".into()),
    }
}

#[cfg(target_os = "linux")]
fn active_plugin_instance(snapshot: &SessionState) -> Result<&PluginInstanceState, String> {
    let instance = snapshot
        .active_instance()
        .ok_or_else(|| "la sesión LIVE no tiene una instancia activa".to_owned())?;
    if !instance.ui_layouts.iter().any(|layout| layout == LITTLE_V1) {
        return Err(format!(
            "{} no declara una vista compatible con {LITTLE_V1}",
            instance.plugin_name
        ));
    }
    Ok(instance)
}

#[cfg(target_os = "linux")]
fn active_plugin_instance_id() -> Result<InstanceId, String> {
    let snapshot = live_snapshot()?;
    Ok(active_plugin_instance(&snapshot)?.instance_id.clone())
}

#[cfg(target_os = "linux")]
fn dispatch_session_command(command: SessionCommand) -> Result<Vec<EventEnvelope>, String> {
    let command_id = NEXT_CONTROL_COMMAND_ID
        .fetch_add(1, Ordering::Relaxed)
        .max(1);
    match control_request(&ControlRequest::Dispatch {
        envelope: CommandEnvelope::new(
            ClientId::new("surface.arturia.keylab-essential-mk3")
                .map_err(|message| format!("client_id inválido: {message}"))?,
            command_id,
            command,
        ),
    })? {
        ControlResponse::CommandApplied {
            command_id: confirmed,
            events,
            ..
        } if confirmed == command_id => Ok(events),
        ControlResponse::Error { message, .. } => Err(message),
        _ => Err("respuesta inesperada al ejecutar comando de sesión".into()),
    }
}

#[cfg(target_os = "linux")]
fn register_host_controls() -> Result<(), String> {
    let profile = controller::package_profile();
    dispatch_session_command(SessionCommand::RegisterHostControls {
        controller_id: profile.driver_id.clone(),
        bindings: profile.host_controls.clone(),
    })?;
    println!(
        "HOST_CONTROLS_RESERVED controller={} count={}",
        profile.driver_id,
        profile.host_controls.len()
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn register_host_controls() -> Result<(), String> {
    Ok(())
}

fn coalesce_host_control_events(
    events: impl IntoIterator<Item = HostControlEvent>,
) -> Vec<HostControlEvent> {
    let mut latest_level = None;
    let mut latest_pan = None;
    let mut last_target = None;
    for event in events {
        match event.target {
            HostControlTarget::MasterLevel => latest_level = Some(event),
            HostControlTarget::MasterPan => latest_pan = Some(event),
        }
        last_target = Some(event.target);
    }

    let mut coalesced = Vec::with_capacity(2);
    match last_target {
        Some(HostControlTarget::MasterLevel) => {
            coalesced.extend(latest_pan);
            coalesced.extend(latest_level);
        }
        Some(HostControlTarget::MasterPan) => {
            coalesced.extend(latest_level);
            coalesced.extend(latest_pan);
        }
        None => {}
    }
    coalesced
}

#[cfg(target_os = "linux")]
fn apply_host_control(event: HostControlEvent) -> Result<(), String> {
    match event.target {
        HostControlTarget::MasterLevel => {
            let level = MasterLevel::from_midi(event.value);
            dispatch_session_command(SessionCommand::SetMasterLevel { level })?;
            println!(
                "MASTER_LEVEL value={} normalized={}/{}",
                event.value,
                level.get(),
                MasterLevel::MAX
            );
            Ok(())
        }
        HostControlTarget::MasterPan => {
            let pan = MasterPan::from_midi_with_center_snap(event.value);
            dispatch_session_command(SessionCommand::SetMasterPan { pan })?;
            println!(
                "MASTER_PAN value={} normalized={}/{}",
                event.value,
                pan.get(),
                MasterPan::MAX
            );
            Ok(())
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_host_control(_event: HostControlEvent) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn refresh_live_catalog(menu: &mut menu::Menu) -> Result<(), String> {
    if let Err(error) = refresh_performance_snapshot(menu) {
        eprintln!("Could not refresh LIVE performance library: {error}");
    }
    if let Err(error) = refresh_audio_settings(menu) {
        eprintln!("Could not refresh CONFIG > AUDIO: {error}");
    }
    if let Err(error) = refresh_web_settings(menu) {
        eprintln!("No se pudo refrescar CONFIG > SYSTEM > WEB: {error}");
    }
    if let Err(error) = refresh_wifi_settings(menu) {
        eprintln!("Could not refresh CONFIG > SYSTEM > WI-FI: {error}");
    }
    let snapshot = live_snapshot()?;
    menu.sync_active_mode(match snapshot.active_mode {
        SurfaceMode::Live => menu::ActiveMode::Live,
        SurfaceMode::Play => menu::ActiveMode::Play,
    });
    let instance = active_plugin_instance(&snapshot)?;
    menu.set_active_plugin(&instance.plugin_id, &instance.plugin_name);
    let selected = instance.selected_sound_id.clone();
    let audition_lease_id = snapshot
        .audition
        .as_ref()
        .filter(|audition| audition.instance_id == instance.instance_id)
        .map(|audition| audition.lease_id);
    let program_draft = snapshot
        .program_draft
        .clone()
        .filter(|draft| draft.instance_id == instance.instance_id);
    menu.sync_program_edit(program_draft, audition_lease_id);
    menu.set_play_sounds(
        instance
            .sounds
            .iter()
            .cloned()
            .map(|sound| {
                menu::PlaySound::new(
                    sound.id,
                    sound.name,
                    sound.bank.unwrap_or_else(|| "default".into()),
                    sound.detail.unwrap_or_else(|| " ".into()),
                )
                .editable(sound.editable)
            })
            .collect(),
        selected.as_deref(),
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn refresh_performance_snapshot(menu: &mut menu::Menu) -> Result<(), String> {
    match control_request(&ControlRequest::PerformanceSnapshot)? {
        ControlResponse::PerformanceSnapshot { snapshot } => {
            menu.sync_performance_snapshot(*snapshot);
            Ok(())
        }
        ControlResponse::Error { message, .. } => Err(message),
        _ => Err("unexpected response while reading LIVE performance state".into()),
    }
}

#[cfg(target_os = "linux")]
fn refresh_audio_settings(menu: &mut menu::Menu) -> Result<(), String> {
    match control_request(&ControlRequest::AudioSnapshot)? {
        ControlResponse::AudioSnapshot { snapshot } => {
            menu.sync_audio_state(*snapshot);
            Ok(())
        }
        ControlResponse::Error { message, .. } => Err(message),
        _ => Err("unexpected response while reading audio state".into()),
    }
}

#[cfg(target_os = "linux")]
fn refresh_wifi_settings(menu: &mut menu::Menu) -> Result<(), String> {
    menu.sync_wifi_settings(load_wifi_settings()?);
    Ok(())
}

#[cfg(target_os = "linux")]
fn load_wifi_settings() -> Result<menu::WifiSystemSettings, String> {
    let status = match platform_control_request(PlatformOperation::GetWifiStatus)? {
        PlatformControlPayload::WifiStatus(status) => status,
        _ => return Err("platform host returned an unexpected Wi-Fi status".into()),
    };
    let saved = match platform_control_request(PlatformOperation::ListSavedWifi)? {
        PlatformControlPayload::SavedWifi(saved) => saved,
        _ => return Err("platform host returned an unexpected saved-network list".into()),
    };
    Ok(menu::WifiSystemSettings {
        available: true,
        enabled: status.enabled,
        connected: status.connected,
        ssid: status.ssid,
        signal_percent: status.signal_percent,
        saved_networks: saved
            .into_iter()
            .map(|connection| menu::SavedWifiNetwork {
                id: connection.id.to_string(),
                name: connection.name,
                ssid: connection.ssid,
                active: connection.active,
            })
            .collect(),
    })
}

#[cfg(target_os = "linux")]
fn platform_control_request(
    operation: PlatformOperation,
) -> Result<PlatformControlPayload, String> {
    let long_operation = matches!(
        &operation,
        PlatformOperation::ScanWifi
            | PlatformOperation::ActivateSavedWifi { .. }
            | PlatformOperation::ForgetSavedWifi { .. }
            | PlatformOperation::ConnectVisibleWifi { .. }
    );
    let request_id = "keylab-platform";
    let request = PlatformControlRequest {
        schema_version: PLATFORM_CONTROL_SCHEMA_VERSION,
        request_id: request_id.into(),
        operation,
    };
    request.validate().map_err(|error| error.to_string())?;
    let path = env::var_os("RACKFORGE_PLATFORM_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(PLATFORM_CONTROL_SOCKET));
    let mut stream =
        UnixStream::connect(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let timeout = if long_operation {
        Duration::from_secs(45)
    } else {
        Duration::from_secs(5)
    };
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut stream, &request).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .take(64 * 1024)
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    match serde_json::from_str::<PlatformControlResponse>(&response)
        .map_err(|error| format!("invalid platform response: {error}"))?
    {
        PlatformControlResponse::Ok {
            schema_version,
            request_id: response_id,
            payload,
        } if schema_version == PLATFORM_CONTROL_SCHEMA_VERSION && response_id == request_id => {
            Ok(payload)
        }
        PlatformControlResponse::Ok { .. } => Err("platform response identity mismatch".into()),
        PlatformControlResponse::Error { code, message, .. } => {
            Err(format!("platform {code:?}: {message}"))
        }
    }
}

#[cfg(target_os = "linux")]
fn refresh_web_settings(menu: &mut menu::Menu) -> Result<(), String> {
    let response = web_control_request(&serde_json::json!({"op": "status"}))?;
    let config = response
        .get("config")
        .ok_or_else(|| "RackForge Web no devolvió su configuración".to_owned())?;
    let enabled = config
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| "RackForge Web no devolvió enabled".to_owned())?;
    let port = config
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "RackForge Web devolvió un puerto inválido".to_owned())?;
    let lan_ip = response
        .get("lan_ip")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<Ipv4Addr>().ok())
        .map(|address| address.octets());
    menu.sync_web_settings(menu::WebSystemSettings {
        enabled,
        access: match config.get("access").and_then(Value::as_str) {
            Some("local") => menu::WebAccess::Local,
            Some("lan") => menu::WebAccess::Lan,
            _ => return Err("RackForge Web devolvió un acceso inválido".into()),
        },
        port,
        lan_ip,
        service_online: true,
        pairing_available: response
            .get("pairing_available")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn web_control_request(request: &Value) -> Result<Value, String> {
    let root = env::var_os("RACKFORGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/kalex/rackforge"));
    let path = root.join("state").join(WEB_CONTROL_SOCKET_NAME);
    let mut stream =
        UnixStream::connect(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(250)))
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut stream, request).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .take(4096)
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    let response: Value = serde_json::from_str(&response)
        .map_err(|error| format!("respuesta WEB inválida: {error}"))?;
    if response.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("RackForge Web devolvió un estado inválido")
            .to_owned());
    }
    Ok(response)
}

#[cfg(not(target_os = "linux"))]
fn refresh_live_catalog(_menu: &mut menu::Menu) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn keep_audition_alive(lease_id: u64) -> Result<(), String> {
    dispatch_session_command(SessionCommand::KeepAuditionAlive { lease_id }).map(|_| ())
}

#[cfg(not(target_os = "linux"))]
fn keep_audition_alive(_lease_id: u64) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_pending_menu_command(
    menu: &mut menu::Menu,
    wifi_task: &mut Option<WifiTask>,
    audio_task: &mut Option<AudioTask>,
) -> Result<bool, String> {
    let Some(command) = menu.take_command() else {
        return Ok(false);
    };
    match command {
        menu::MenuCommand::SetActiveMode { mode } => {
            dispatch_session_command(SessionCommand::SetActiveMode {
                mode: match mode {
                    menu::ActiveMode::Live => SurfaceMode::Live,
                    menu::ActiveMode::Play => SurfaceMode::Play,
                },
            })?;
            println!("ACTIVE_MODE_SET mode={mode:?}");
            Ok(true)
        }
        menu::MenuCommand::SetLiveBrowseMode { mode } => {
            dispatch_session_command(SessionCommand::SetLiveBrowseMode { mode })?;
            refresh_live_catalog(menu)?;
            println!("LIVE_BROWSE_MODE_SET mode={mode:?}");
            Ok(true)
        }
        menu::MenuCommand::ActivateLiveTarget { location } => {
            dispatch_session_command(SessionCommand::ActivateLiveTarget {
                location: location.clone(),
            })?;
            refresh_live_catalog(menu)?;
            println!("LIVE_TARGET_ACTIVATED location={location:?}");
            Ok(true)
        }
        menu::MenuCommand::PreviewRack { rack } => {
            let rack_id = rack.id.clone();
            let slot_count = rack.slots.iter().filter(|slot| slot.enabled).count();
            dispatch_session_command(SessionCommand::PreviewRack { rack })?;
            println!("RACK_PREVIEWED id={rack_id} slots={slot_count}");
            Ok(true)
        }
        menu::MenuCommand::EditPerformance {
            expected_revision,
            edit,
        } => {
            let result = match control_request(&ControlRequest::EditPerformance {
                expected_revision,
                edit,
            }) {
                Ok(ControlResponse::PerformanceEdited { snapshot }) => Ok(*snapshot),
                Ok(ControlResponse::Error { message, .. }) => Err(message),
                Ok(_) => Err("unexpected response while editing performance library".into()),
                Err(error) => Err(error),
            };
            let success = result.is_ok();
            menu.complete_performance_edit(result);
            println!("PERFORMANCE_EDIT_COMPLETED success={success}");
            Ok(true)
        }
        menu::MenuCommand::SelectSound { id } => {
            let instance_id = active_plugin_instance_id()?;
            dispatch_session_command(SessionCommand::SelectSound {
                instance_id,
                sound_id: id.clone(),
            })?;
            refresh_live_catalog(menu)?;
            println!("SOUND_SELECTED id={id}");
            Ok(true)
        }
        menu::MenuCommand::BeginProgramEdit { program_id } => {
            let instance_id = active_plugin_instance_id()?;
            dispatch_session_command(SessionCommand::BeginProgramEdit {
                instance_id,
                program_id,
            })?;
            refresh_live_catalog(menu)?;
            let draft_id = live_snapshot()?
                .program_draft
                .as_ref()
                .map(|draft| draft.draft_id)
                .ok_or_else(|| "RackForge no publicó el borrador de programa".to_owned())?;
            println!("PROGRAM_EDIT_STARTED draft={draft_id}");
            Ok(true)
        }
        menu::MenuCommand::EditProgramDraftField {
            draft_id,
            field_id,
            value,
            preview,
        } => {
            dispatch_session_command(SessionCommand::EditProgramDraftField {
                draft_id,
                field_id: field_id.clone(),
                value,
                preview,
            })?;
            if !preview {
                refresh_live_catalog(menu)?;
            }
            println!("PROGRAM_DRAFT_FIELD draft={draft_id} field={field_id} preview={preview}");
            Ok(true)
        }
        menu::MenuCommand::RestoreProgramDraftPreview { draft_id } => {
            dispatch_session_command(SessionCommand::RestoreProgramDraftPreview { draft_id })?;
            println!("PROGRAM_DRAFT_PREVIEW_RESTORED draft={draft_id}");
            Ok(true)
        }
        menu::MenuCommand::SetProgramDraftName { draft_id, name } => {
            let snapshot = live_snapshot()?;
            let draft = snapshot
                .program_draft
                .as_ref()
                .filter(|draft| draft.draft_id == draft_id)
                .ok_or_else(|| "el borrador de programa ya no está activo".to_owned())?;
            let document_json = replace_program_name(&draft.document_json, &name)?;
            dispatch_session_command(SessionCommand::ReplaceProgramDraft {
                draft_id,
                document_json,
            })?;
            refresh_live_catalog(menu)?;
            println!("PROGRAM_DRAFT_NAME draft={draft_id} name={name:?}");
            Ok(true)
        }
        menu::MenuCommand::SaveProgramDraft { draft_id } => {
            dispatch_session_command(SessionCommand::SaveProgramDraft { draft_id })?;
            refresh_live_catalog(menu)?;
            println!("PROGRAM_SAVED draft={draft_id}");
            Ok(true)
        }
        menu::MenuCommand::CancelProgramEdit { draft_id } => {
            dispatch_session_command(SessionCommand::CancelProgramEdit { draft_id })?;
            refresh_live_catalog(menu)?;
            println!("PROGRAM_EDIT_CANCELLED draft={draft_id}");
            Ok(true)
        }
        menu::MenuCommand::ResolveProgramExit {
            draft_id,
            decision,
            destination,
        } => {
            match decision {
                menu::ProgramExitDecision::Save => {
                    dispatch_session_command(SessionCommand::SaveProgramDraft { draft_id })?;
                    println!("PROGRAM_SAVED_ON_EXIT draft={draft_id}");
                }
                menu::ProgramExitDecision::Discard => {
                    dispatch_session_command(SessionCommand::CancelProgramEdit { draft_id })?;
                    println!("PROGRAM_DISCARDED_ON_EXIT draft={draft_id}");
                }
            }
            refresh_live_catalog(menu)?;
            if let menu::ProgramExitDestination::ActiveMode {
                mode,
                selected_sound_id,
            } = destination
            {
                return_to_active_mode(menu, mode, selected_sound_id)?;
            }
            Ok(true)
        }
        menu::MenuCommand::ReturnToActiveMode {
            mode,
            cancel_draft_id,
            selected_sound_id,
        } => {
            if let Some(draft_id) = cancel_draft_id {
                dispatch_session_command(SessionCommand::CancelProgramEdit { draft_id })?;
            }
            refresh_live_catalog(menu)?;
            return_to_active_mode(menu, mode, selected_sound_id)?;
            Ok(true)
        }
        menu::MenuCommand::SetWebEnabled { enabled } => {
            web_control_request(&serde_json::json!({
                "op": "set",
                "field": "enabled",
                "value": enabled
            }))?;
            println!("WEB_SETTING_SET field=enabled value={enabled}");
            Ok(true)
        }
        menu::MenuCommand::SetWebAccess { access } => {
            let value = match access {
                menu::WebAccess::Local => "local",
                menu::WebAccess::Lan => "lan",
            };
            web_control_request(&serde_json::json!({
                "op": "set",
                "field": "access",
                "value": value
            }))?;
            println!("WEB_SETTING_SET field=access value={value}");
            Ok(true)
        }
        menu::MenuCommand::SetWebPort { port } => {
            web_control_request(&serde_json::json!({
                "op": "set",
                "field": "port",
                "value": port
            }))?;
            println!("WEB_SETTING_SET field=port value={port}");
            Ok(true)
        }
        menu::MenuCommand::BeginWebPairing => {
            let response = web_control_request(&serde_json::json!({"op": "begin_pairing"}))?;
            let code = response
                .get("pairing_code")
                .and_then(Value::as_str)
                .ok_or_else(|| "RackForge Web no devolvió el código".to_owned())?;
            menu.show_pairing_code(code);
            println!("WEB_PAIRING_STARTED");
            Ok(true)
        }
        menu::MenuCommand::ActivateSavedWifi { connection_id } => {
            let connection_id =
                WifiConnectionId::new(connection_id).map_err(|error| error.to_string())?;
            start_wifi_change(
                wifi_task,
                menu,
                "CONNECTING",
                "CONNECTED",
                PlatformOperation::ActivateSavedWifi { connection_id },
            )?;
            Ok(true)
        }
        menu::MenuCommand::ForgetSavedWifi { connection_id } => {
            let connection_id =
                WifiConnectionId::new(connection_id).map_err(|error| error.to_string())?;
            start_wifi_change(
                wifi_task,
                menu,
                "FORGETTING",
                "FORGOTTEN",
                PlatformOperation::ForgetSavedWifi { connection_id },
            )?;
            Ok(true)
        }
        menu::MenuCommand::ConnectDiscoveredWifi { ssid, passphrase } => {
            let ssid = WifiSsid::new(ssid).map_err(|error| error.to_string())?;
            let passphrase = passphrase
                .as_ref()
                .map(|secret| WifiPassphrase::new(secret.expose()))
                .transpose()
                .map_err(|error| error.to_string())?;
            start_wifi_change(
                wifi_task,
                menu,
                "CONNECTING",
                "CONNECTED",
                PlatformOperation::ConnectVisibleWifi { ssid, passphrase },
            )?;
            Ok(true)
        }
        menu::MenuCommand::DisconnectWifi => {
            start_wifi_change(
                wifi_task,
                menu,
                "DISCONNECTING",
                "DISCONNECTED",
                PlatformOperation::DisconnectWifi,
            )?;
            Ok(true)
        }
        menu::MenuCommand::SetWifiEnabled { enabled } => {
            start_wifi_change(
                wifi_task,
                menu,
                "UPDATING RADIO",
                if enabled { "RADIO ON" } else { "RADIO OFF" },
                PlatformOperation::SetWifiEnabled { enabled },
            )?;
            Ok(true)
        }
        menu::MenuCommand::ScanWifi => {
            start_wifi_scan(wifi_task, menu)?;
            Ok(true)
        }
        menu::MenuCommand::ApplyAudioOutput { profile } => {
            start_audio_change(audio_task, menu, profile)?;
            Ok(true)
        }
        menu::MenuCommand::ForceHome { cancel_draft_id } => {
            if let Some(draft_id) = cancel_draft_id
                && let Err(error) =
                    dispatch_session_command(SessionCommand::CancelProgramEdit { draft_id })
            {
                eprintln!(
                    "HOME_FORCED: el Core no liberó inmediatamente el draft {draft_id}: {error}"
                );
            }
            println!("HOME_FORCED");
            Ok(true)
        }
    }
}

#[cfg(target_os = "linux")]
fn wifi_error_summary(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.contains("password") || lower.contains("secret") || lower.contains("authentication") {
        "AUTH FAILED"
    } else if lower.contains("not found") {
        "NOT FOUND"
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "TIMEOUT"
    } else {
        "CONNECTION ERROR"
    }
}

#[cfg(target_os = "linux")]
fn start_wifi_change(
    task: &mut Option<WifiTask>,
    menu: &mut menu::Menu,
    busy_label: &'static str,
    success_message: &'static str,
    operation: PlatformOperation,
) -> Result<(), String> {
    if task.is_some() {
        return Err("a Wi-Fi operation is already running".into());
    }
    let (sender, receiver) = mpsc::channel();
    menu.begin_wifi_busy(busy_label);
    *task = Some(WifiTask { receiver });
    thread::spawn(move || {
        let result = platform_control_request(operation)
            .and_then(|_| load_wifi_settings())
            .map(|settings| WifiTaskSuccess::Changed {
                message: success_message,
                settings,
            });
        let _ = sender.send(result);
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn start_wifi_scan(task: &mut Option<WifiTask>, menu: &mut menu::Menu) -> Result<(), String> {
    if task.is_some() {
        return Err("a Wi-Fi operation is already running".into());
    }
    let (sender, receiver) = mpsc::channel();
    menu.begin_wifi_busy("SCANNING");
    *task = Some(WifiTask { receiver });
    thread::spawn(move || {
        let result = platform_control_request(PlatformOperation::ScanWifi).and_then(|payload| {
            let PlatformControlPayload::VisibleWifi(networks) = payload else {
                return Err("platform host returned an unexpected Wi-Fi scan".into());
            };
            let networks = networks
                .into_iter()
                .map(|network| menu::DiscoveredWifiNetwork {
                    ssid: network.ssid,
                    signal_percent: network.signal_percent,
                    secured: network.secured,
                })
                .collect();
            Ok(WifiTaskSuccess::Scan {
                networks,
                settings: load_wifi_settings()?,
            })
        });
        let _ = sender.send(result);
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn poll_wifi_task(task: &mut Option<WifiTask>, menu: &mut menu::Menu) -> bool {
    let Some(active) = task.as_ref() else {
        return false;
    };
    let result = match active.receiver.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return false,
        Err(mpsc::TryRecvError::Disconnected) => {
            Err("Wi-Fi worker stopped without a result".into())
        }
    };
    *task = None;
    match result {
        Ok(WifiTaskSuccess::Scan { networks, settings }) => {
            let count = networks.len();
            menu.sync_wifi_settings(settings);
            menu.complete_wifi_scan(networks);
            println!("WIFI_SCAN_COMPLETED count={count}");
        }
        Ok(WifiTaskSuccess::Changed { message, settings }) => {
            menu.sync_wifi_settings(settings);
            menu.complete_wifi_result(true, message);
            println!("WIFI_OPERATION_COMPLETED result={message:?}");
        }
        Err(error) => {
            menu.complete_wifi_result(false, wifi_error_summary(&error));
            eprintln!("WIFI_OPERATION_FAILED: {error}");
        }
    }
    true
}

#[cfg(not(target_os = "linux"))]
fn poll_wifi_task(_task: &mut Option<WifiTask>, _menu: &mut menu::Menu) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn start_audio_change(
    task: &mut Option<AudioTask>,
    menu: &mut menu::Menu,
    profile: rackforge_control_api::AudioOutputProfile,
) -> Result<(), String> {
    if task.is_some() {
        return Err("an audio change is already running".into());
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    *task = Some(AudioTask { receiver });
    menu.begin_audio_busy();
    thread::Builder::new()
        .name("rackforge-audio-settings".into())
        .spawn(move || {
            let result = match control_request_with_timeout(
                &ControlRequest::ApplyAudioOutput { profile },
                Duration::from_secs(10),
            ) {
                Ok(ControlResponse::AudioApplied { snapshot }) => Ok(*snapshot),
                Ok(ControlResponse::Error { message, .. }) => Err(message),
                Ok(_) => Err("unexpected response while applying audio output".into()),
                Err(error) => Err(error),
            };
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn poll_audio_task(task: &mut Option<AudioTask>, menu: &mut menu::Menu) -> bool {
    let Some(active) = task else {
        return false;
    };
    let result = match active.receiver.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return false,
        Err(mpsc::TryRecvError::Disconnected) => {
            Err("audio settings worker stopped unexpectedly".into())
        }
    };
    *task = None;
    if let Err(error) = &result {
        eprintln!("AUDIO_CHANGE_FAILED {error}");
    }
    menu.complete_audio_change(result);
    true
}

#[cfg(not(target_os = "linux"))]
fn poll_audio_task(_task: &mut Option<AudioTask>, _menu: &mut menu::Menu) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn return_to_active_mode(
    menu: &mut menu::Menu,
    mode: menu::ActiveMode,
    selected_sound_id: Option<String>,
) -> Result<(), String> {
    let instance_id = active_plugin_instance_id()?;
    let surface_mode = match mode {
        menu::ActiveMode::Live => SurfaceMode::Live,
        menu::ActiveMode::Play => SurfaceMode::Play,
    };
    let events = dispatch_session_command(SessionCommand::ActivateSurface {
        instance_id,
        request: SurfaceActivationRequest::return_to(
            LITTLE_V1,
            surface_mode,
            selected_sound_id.clone(),
        ),
    })?;
    let focus_item_id = events.iter().find_map(|event| match &event.event {
        SessionEvent::SurfaceActivated { response, .. } => response.focus_item_id.as_deref(),
        _ => None,
    });
    menu.complete_return_to_active_mode(mode, focus_item_id.or(selected_sound_id.as_deref()));
    println!("SURFACE_RETURNED mode={mode:?}");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn apply_pending_menu_command(
    menu: &mut menu::Menu,
    _wifi_task: &mut Option<WifiTask>,
    _audio_task: &mut Option<AudioTask>,
) -> Result<bool, String> {
    match menu.take_command() {
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

#[cfg(target_os = "linux")]
fn replace_program_name(document_json: &str, name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 64 || !name.is_ascii() || name.contains('\0') {
        return Err("el nombre debe tener entre 1 y 64 caracteres ASCII".into());
    }
    let mut document: Value = serde_json::from_str(document_json)
        .map_err(|error| format!("el borrador contiene JSON inválido: {error}"))?;
    let object = document
        .as_object_mut()
        .ok_or_else(|| "el borrador de programa no es un objeto JSON".to_owned())?;
    object.insert("name".into(), Value::from(name));
    serde_json::to_string(&document)
        .map_err(|error| format!("no se pudo serializar el borrador plugin: {error}"))
}

fn acquire_screen(
    session: &mut KeyLabSession,
    messages: &MenuMessages,
    ack_receiver: &Receiver<()>,
    usb_generation: Option<&str>,
) -> Result<bool, Box<dyn Error>> {
    let mut attempt = 0_u64;
    loop {
        if usb_generation.is_some() && keylab_usb_generation().as_deref() != usb_generation {
            return Ok(false);
        }
        attempt += 1;
        drain_acks(ack_receiver);
        session.start()?;
        send_menu(session, messages)?;
        match ack_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(()) => {
                println!("OLED_ACK recibido después de {attempt} pulso(s)");
                return Ok(true);
            }
            Err(RecvTimeoutError::Timeout) => {
                eprintln!("Pulso OLED {attempt} sin ACK; reintentando...");
                thread::sleep(ACQUIRE_RETRY_DELAY);
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err("Se cerró el canal de confirmación MIDI".into());
            }
        }
    }
}

fn open_keylab_input(selector: Option<&str>) -> Result<KeyLabInput, Box<dyn Error>> {
    let mut midi = MidiInput::new("rackforge KeyLab Display ACK")?;
    midi.ignore(Ignore::None);
    let ports = enumerate_input_ports(&midi)?;
    let port = select_input_port(&ports, selector)?;
    let (ack_sender, ack_receiver) = mpsc::channel();
    let (input_sender, input_receiver) = mpsc::channel();
    let (host_control_sender, host_control_receiver) = mpsc::channel();
    let host_controls = controller::package_profile().host_controls.clone();
    let connection = midi.connect(
        &port.handle,
        "rackforge KeyLab DAW ACK",
        move |_timestamp, message, _context| {
            if is_daw_preset_ack(message) {
                let _ = ack_sender.send(());
            }
            if let Some(input) = parse_physical_input(message) {
                let _ = input_sender.send(input);
            }
            if let Some(input) = parse_host_control(message, &host_controls) {
                let _ = host_control_sender.send(input);
            }
        },
        (),
    )?;
    Ok(KeyLabInput {
        _connection: connection,
        ack_receiver,
        input_receiver,
        host_control_receiver,
    })
}

fn parse_host_control(message: &[u8], bindings: &[HostControlBinding]) -> Option<HostControlEvent> {
    bindings.iter().find_map(|binding| {
        binding
            .midi_cc
            .value(message)
            .map(|value| HostControlEvent {
                target: binding.target,
                value,
            })
    })
}

fn parse_physical_input(message: &[u8]) -> Option<PhysicalInputEvent> {
    let [0xB0, controller, value] = message else {
        return None;
    };
    match (*controller, *value) {
        (44, 127) => Some(input_event(menu::Input::Button1, InputPhase::Press)),
        (45, 127) => Some(input_event(menu::Input::Button2, InputPhase::Press)),
        (46, 127) => Some(input_event(menu::Input::Button3, InputPhase::Press)),
        (47, 127) => Some(input_event(menu::Input::Button4, InputPhase::Press)),
        (44, 0) => Some(input_event(menu::Input::Button1, InputPhase::Release)),
        (45, 0) => Some(input_event(menu::Input::Button2, InputPhase::Release)),
        (46, 0) => Some(input_event(menu::Input::Button3, InputPhase::Release)),
        (47, 0) => Some(input_event(menu::Input::Button4, InputPhase::Release)),
        (116, 0..=63) => Some(input_event(menu::Input::EncoderLeft, InputPhase::Turn)),
        (116, 65..=127) => Some(input_event(menu::Input::EncoderRight, InputPhase::Turn)),
        (117, 127) => Some(input_event(menu::Input::EncoderPress, InputPhase::Press)),
        (117, 0) => Some(input_event(menu::Input::EncoderPress, InputPhase::Release)),
        _ => None,
    }
}

fn input_event(input: menu::Input, phase: InputPhase) -> PhysicalInputEvent {
    PhysicalInputEvent { input, phase }
}

struct MenuMessages {
    header: Vec<u8>,
    body: Vec<u8>,
    footer: Vec<u8>,
}

fn render_menu_messages(menu: &menu::Menu) -> Result<MenuMessages, String> {
    render_screen_messages(&menu.render())
}

fn render_screen_messages(screen: &menu::Screen) -> Result<MenuMessages, String> {
    Ok(MenuMessages {
        header: screen_header_message(&screen.header)?,
        body: two_lines(&screen.line_1, &screen.line_2)?,
        footer: footer(&screen.footer)?,
    })
}

fn screen_header_message(header_mode: &menu::Header) -> Result<Vec<u8>, String> {
    match header_mode {
        menu::Header::Visible(title) => header(title),
        menu::Header::Hidden => header(" "),
    }
}

fn send_menu(session: &mut KeyLabSession, messages: &MenuMessages) -> Result<(), Box<dyn Error>> {
    send_menu_with_header_override(session, messages, None)
}

fn send_menu_with_header_override(
    session: &mut KeyLabSession,
    messages: &MenuMessages,
    header_override: Option<&[u8]>,
) -> Result<(), Box<dyn Error>> {
    session.send(&messages.body)?;
    thread::sleep(Duration::from_millis(20));
    session.send(header_override.unwrap_or(&messages.header))?;
    thread::sleep(Duration::from_millis(20));
    session.send(&messages.footer)
}

fn send_menu_frames(
    session: &mut KeyLabSession,
    screens: Vec<menu::Screen>,
    header_override: Option<&[u8]>,
) -> Result<MenuMessages, Box<dyn Error>> {
    let frame_count = screens.len();
    let mut last = None;
    for (index, screen) in screens.into_iter().enumerate() {
        let messages = render_screen_messages(&screen)?;
        send_menu_with_header_override(session, &messages, header_override)?;
        if index + 1 < frame_count {
            thread::sleep(Duration::from_millis(55));
        }
        last = Some(messages);
    }
    last.ok_or_else(|| "La transición del menú no produjo frames".into())
}

fn is_daw_preset_ack(message: &[u8]) -> bool {
    message
        == [
            0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x21, 0x11, 0x40, 0x02, 0x00, 0x01, 0xF7,
        ]
}

fn drain_acks(receiver: &Receiver<()>) {
    while receiver.try_recv().is_ok() {}
}

fn verify_daw_ack(
    session: &mut KeyLabSession,
    receiver: &Receiver<()>,
) -> Result<bool, Box<dyn Error>> {
    drain_acks(receiver);
    session.send(&select_preset(1)?)?;
    match receiver.recv_timeout(Duration::from_millis(750)) {
        Ok(()) => Ok(true),
        Err(RecvTimeoutError::Timeout) => Ok(false),
        Err(RecvTimeoutError::Disconnected) => Err("Se cerró el canal de heartbeat MIDI".into()),
    }
}

fn wait_for_keylab_usb(expected: &str, duration: Duration) -> bool {
    let already_stable = keylab_usb_age().unwrap_or_default();
    let remaining = duration.saturating_sub(already_stable);
    if !remaining.is_zero() {
        println!(
            "Protección USB: faltan {:.2}s de estabilidad del KeyLab...",
            remaining.as_secs_f64()
        );
    }
    let deadline = Instant::now() + remaining;
    while Instant::now() < deadline {
        if keylab_usb_generation().as_deref() != Some(expected) {
            return false;
        }
        thread::sleep(Duration::from_millis(250));
    }
    true
}

#[cfg(target_os = "linux")]
fn keylab_usb_age() -> Option<Duration> {
    let devices = fs::read_dir("/sys/bus/usb/devices").ok()?;
    for entry in devices.flatten() {
        let path = entry.path();
        let vendor = fs::read_to_string(path.join("idVendor")).ok();
        let product = fs::read_to_string(path.join("idProduct")).ok();
        if vendor.as_deref().map(str::trim) != Some("1c75")
            || product.as_deref().map(str::trim) != Some("028c")
        {
            continue;
        }
        let busnum = fs::read_to_string(path.join("busnum"))
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()?;
        let devnum = fs::read_to_string(path.join("devnum"))
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()?;
        let minor = busnum.checked_sub(1)?.checked_mul(128)? + devnum.checked_sub(1)?;
        let udev_data = fs::read_to_string(format!("/run/udev/data/c189:{minor}")).ok()?;
        let initialized = parse_udev_initialized(&udev_data)?;
        return boot_uptime()?.checked_sub(initialized);
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn keylab_usb_age() -> Option<Duration> {
    None
}

fn parse_udev_initialized(data: &str) -> Option<Duration> {
    let micros = data
        .lines()
        .find_map(|line| line.strip_prefix("I:"))?
        .parse::<u64>()
        .ok()?;
    Some(Duration::from_micros(micros))
}

#[cfg(target_os = "linux")]
fn boot_uptime() -> Option<Duration> {
    let uptime = fs::read_to_string("/proc/uptime").ok()?;
    let seconds = uptime.split_whitespace().next()?.parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds.is_sign_negative() {
        return None;
    }
    Some(Duration::from_secs_f64(seconds))
}

#[cfg(target_os = "linux")]
fn keylab_usb_generation() -> Option<String> {
    let devices = fs::read_dir("/sys/bus/usb/devices").ok()?;
    for entry in devices.flatten() {
        let path = entry.path();
        let vendor = fs::read_to_string(path.join("idVendor")).ok();
        let product = fs::read_to_string(path.join("idProduct")).ok();
        if vendor.as_deref().map(str::trim) != Some("1c75")
            || product.as_deref().map(str::trim) != Some("028c")
        {
            continue;
        }
        let devnum = fs::read_to_string(path.join("devnum")).ok()?;
        return Some(format!(
            "{}:{}",
            entry.file_name().to_string_lossy(),
            devnum.trim()
        ));
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn keylab_usb_generation() -> Option<String> {
    Some("non-linux".to_owned())
}

fn run_restore(midi: MidiOutput, port: &PortInfo, execute: bool) -> Result<(), Box<dyn Error>> {
    println!("Puerto: [{}] {}", port.index, port.name);
    if !execute {
        println!("DRY-RUN: se restauraría la pantalla y el programa Arturia.");
        return Ok(());
    }

    let mut session = KeyLabSession::open(midi, port)?;
    session.send(CLEAR_SCREEN)?;
    session.send(DISCONNECT)?;
    thread::sleep(Duration::from_millis(150));
    session.send(&select_preset(0)?)?;
    println!("Pantalla y programa Arturia restaurados.");
    Ok(())
}

fn run_led_demo(midi: MidiOutput, port: &PortInfo, execute: bool) -> Result<(), Box<dyn Error>> {
    println!("Puerto: [{}] {}", port.index, port.name);
    if !execute {
        println!("DRY-RUN: se probarían temporalmente CC 44–47 y luego quedarían apagados.");
        return Ok(());
    }
    if !is_keylab_midi(&port.name) {
        return Err("El puerto seleccionado no es el MIDI principal del KeyLab".into());
    }

    let mut connection = midi.connect(&port.handle, "rackforge KeyLab LED probe")?;
    let result = (|| -> Result<(), Box<dyn Error>> {
        for controller in 44..=47 {
            connection.send(&[0xB0, controller, 0])?;
        }
        for controller in 44..=47 {
            connection.send(&[0xB0, controller, 127])?;
            thread::sleep(Duration::from_millis(700));
            connection.send(&[0xB0, controller, 0])?;
            thread::sleep(Duration::from_millis(150));
        }
        for controller in 44..=47 {
            connection.send(&[0xB0, controller, 127])?;
        }
        thread::sleep(Duration::from_secs(1));
        Ok(())
    })();
    for controller in 44..=47 {
        if let Err(error) = connection.send(&[0xB0, controller, 0]) {
            eprintln!("No se pudo apagar LED CC {controller}: {error}");
        }
    }
    result?;
    println!("Prueba LED finalizada; CC 44–47 quedaron en cero.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_udev_usb_initialization_timestamp() {
        assert_eq!(
            parse_udev_initialized("I:1828193\nE:ID_BUS=usb\n"),
            Some(Duration::from_micros(1_828_193))
        );
        assert_eq!(parse_udev_initialized("E:ID_BUS=usb\n"), None);
        assert_eq!(parse_udev_initialized("I:not-a-number\n"), None);
    }

    #[test]
    fn button_gestures_resolve_short_and_long_without_double_firing() {
        let started = Instant::now();
        let mut tracker = ButtonGestureTracker::default();
        assert!(tracker.press(menu::Input::Button2, started));
        assert_eq!(
            tracker.release(menu::Input::Button2, started + Duration::from_millis(100)),
            Some(menu::Input::Button2)
        );

        assert!(tracker.press(menu::Input::Button2, started));
        assert!(
            tracker
                .poll(started + LONG_PRESS_THRESHOLD - Duration::from_millis(1))
                .is_empty()
        );
        assert_eq!(
            tracker.poll(started + LONG_PRESS_THRESHOLD),
            vec![menu::Input::Button2Long]
        );
        assert!(
            tracker
                .release(
                    menu::Input::Button2,
                    started + LONG_PRESS_THRESHOLD + Duration::from_millis(1)
                )
                .is_none()
        );
    }

    #[test]
    fn simultaneous_long_ok_and_back_has_priority_as_home_chord() {
        let started = Instant::now();
        let mut tracker = ButtonGestureTracker::default();
        tracker.press(menu::Input::Button1, started);
        tracker.press(menu::Input::Button4, started + Duration::from_millis(100));
        assert_eq!(
            tracker.poll(started + Duration::from_millis(750)),
            vec![menu::Input::HomeChord]
        );
        assert!(
            tracker
                .release(menu::Input::Button1, started + Duration::from_millis(760))
                .is_none()
        );
        assert!(
            tracker
                .release(menu::Input::Button4, started + Duration::from_millis(770))
                .is_none()
        );
    }

    #[test]
    fn builds_expected_preset_messages() {
        assert_eq!(
            select_preset(1).unwrap(),
            [
                0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x21, 0x11, 0x40, 0x02, 0x00, 0x01, 0xF7
            ]
        );
        assert_eq!(
            select_preset(0).unwrap(),
            [
                0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x21, 0x11, 0x40, 0x02, 0x00, 0x00, 0xF7
            ]
        );
    }

    #[test]
    fn builds_expected_screen_message() {
        assert_eq!(
            two_lines("DOOM", "RUST").unwrap(),
            [
                0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x60, 0x12, 0x01, b'D', b'O', b'O',
                b'M', 0x00, 0x02, b'R', b'U', b'S', b'T', 0x00, 0x00, 0xF7
            ]
        );
    }

    #[test]
    fn builds_native_contextual_footer() {
        let menu = menu::Menu::default();
        assert_eq!(
            footer(&menu.render().footer).unwrap(),
            [
                0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x60, 0x03, 0x10, 0x00, 0x00, 0x11,
                b'O', b'K', 0x00, 0x20, 0x00, 0x00, 0x21, b'<', 0x00, 0x30, 0x00, 0x00, 0x31, b'>',
                0x00, 0x40, 0x00, 0x00, 0x41, b'B', b'A', b'C', b'K', 0x00, 0xF7
            ]
        );
    }

    #[test]
    fn pressed_footer_button_uses_the_full_frame() {
        let mut menu = menu::Menu::default();
        menu.set_button_pressed(menu::Input::Button1, true);
        let message = footer(&menu.render().footer).unwrap();
        assert_eq!(&message[10..14], &[0x10, 0x03, 0x00, 0x11]);
    }

    #[test]
    fn only_accepts_the_dedicated_midi_port() {
        assert!(is_keylab_midi("KL Essential 61 mk3 MIDI"));
        assert!(is_keylab_midi(
            "KL Essential 61 mk3:KL Essential 61 mk3 MIDI 28:0"
        ));
        assert!(!is_keylab_midi("KL Essential 61 mk3 MCU/HUI"));
        assert!(!is_keylab_midi(
            "KL Essential 61 mk3:KL Essential 61 mk3 DINTHRU 28:1"
        ));
        assert!(!is_keylab_midi(
            "KL Essential 61 mk3:KL Essential 61 mk3 MCU/HUI 28:2"
        ));
        assert!(!is_keylab_midi("KL Essential 61 mk3 ALV"));
    }

    #[test]
    fn rejects_unsafe_screen_text() {
        assert!(two_lines("DOOM", "Ñ").is_err());
        assert!(two_lines("DOOM", "X".repeat(19).as_str()).is_err());
        let mut screen = menu::Menu::default().render();
        screen.footer[3].label = "TOO-LONG".into();
        assert!(footer(&screen.footer).is_err());
    }

    #[test]
    fn recognizes_only_the_daw_program_ack() {
        assert!(is_daw_preset_ack(&select_preset(1).unwrap()));
        assert!(!is_daw_preset_ack(&select_preset(0).unwrap()));
        assert!(!is_daw_preset_ack(&[0x90, 60, 100]));
    }

    #[test]
    fn maps_the_seven_captured_keylab_inputs() {
        assert_eq!(
            parse_physical_input(&[0xB0, 44, 127]),
            Some(input_event(menu::Input::Button1, InputPhase::Press))
        );
        assert_eq!(
            parse_physical_input(&[0xB0, 45, 127]),
            Some(input_event(menu::Input::Button2, InputPhase::Press))
        );
        assert_eq!(
            parse_physical_input(&[0xB0, 46, 127]),
            Some(input_event(menu::Input::Button3, InputPhase::Press))
        );
        assert_eq!(
            parse_physical_input(&[0xB0, 47, 127]),
            Some(input_event(menu::Input::Button4, InputPhase::Press))
        );
        assert_eq!(
            parse_physical_input(&[0xB0, 116, 62]),
            Some(input_event(menu::Input::EncoderLeft, InputPhase::Turn))
        );
        assert_eq!(
            parse_physical_input(&[0xB0, 116, 66]),
            Some(input_event(menu::Input::EncoderRight, InputPhase::Turn))
        );
        assert_eq!(
            parse_physical_input(&[0xB0, 117, 127]),
            Some(input_event(menu::Input::EncoderPress, InputPhase::Press))
        );
    }

    #[test]
    fn captures_releases_and_ignores_encoder_neutral() {
        assert_eq!(
            parse_physical_input(&[0xB0, 44, 0]),
            Some(input_event(menu::Input::Button1, InputPhase::Release))
        );
        assert_eq!(
            parse_physical_input(&[0xB0, 117, 0]),
            Some(input_event(menu::Input::EncoderPress, InputPhase::Release))
        );
        assert_eq!(parse_physical_input(&[0xB0, 116, 64]), None);
        assert_eq!(parse_physical_input(&[0x90, 60, 100]), None);
    }

    #[test]
    fn reserved_host_control_is_separate_from_surface_navigation() {
        let bindings = [
            HostControlBinding {
                target: HostControlTarget::MasterLevel,
                midi_cc: rackforge_session_api::MidiControlChangeBinding {
                    channel: 0,
                    controller: 82,
                },
            },
            HostControlBinding {
                target: HostControlTarget::MasterPan,
                midi_cc: rackforge_session_api::MidiControlChangeBinding {
                    channel: 0,
                    controller: 104,
                },
            },
        ];
        assert_eq!(
            parse_host_control(&[0xb0, 82, 91], &bindings),
            Some(HostControlEvent {
                target: HostControlTarget::MasterLevel,
                value: 91,
            })
        );
        assert_eq!(
            parse_host_control(&[0xb0, 104, 64], &bindings),
            Some(HostControlEvent {
                target: HostControlTarget::MasterPan,
                value: 64,
            })
        );
        assert_eq!(parse_host_control(&[0xb0, 83, 91], &bindings), None);
        assert_eq!(parse_physical_input(&[0xb0, 82, 91]), None);
    }

    #[test]
    fn host_control_feedback_fits_the_native_header_and_snaps_pan_to_center() {
        assert_eq!(
            HostControlEvent {
                target: HostControlTarget::MasterLevel,
                value: 0,
            }
            .header_text(),
            "MASTER VOL      0%"
        );
        assert_eq!(
            HostControlEvent {
                target: HostControlTarget::MasterLevel,
                value: 127,
            }
            .header_text(),
            "MASTER VOL    100%"
        );
        assert_eq!(
            HostControlEvent {
                target: HostControlTarget::MasterPan,
                value: 64,
            }
            .header_text(),
            "MASTER PAN  CENTER"
        );
        assert_eq!(
            HostControlEvent {
                target: HostControlTarget::MasterPan,
                value: 0,
            }
            .header_text(),
            "MASTER PAN  L 100%"
        );
        assert_eq!(
            HostControlEvent {
                target: HostControlTarget::MasterPan,
                value: 127,
            }
            .header_text(),
            "MASTER PAN  R 100%"
        );
    }

    #[test]
    fn transient_header_refreshes_its_deadline_and_restores_after_inactivity() {
        let start = Instant::now();
        let mut transient = TransientHeader::default();
        let first = header("MASTER VOL     50%").unwrap();
        transient.show("MASTER VOL     50%", start).unwrap();
        assert_eq!(
            transient.visible_message(start + Duration::from_millis(1_499)),
            Some(first.as_slice())
        );

        let refreshed_at = start + Duration::from_millis(1_000);
        let second = header("MASTER PAN  CENTER").unwrap();
        transient.show("MASTER PAN  CENTER", refreshed_at).unwrap();
        assert!(!transient.expire(start + Duration::from_millis(1_501)));
        assert_eq!(
            transient.visible_message(refreshed_at + Duration::from_millis(1_499)),
            Some(second.as_slice())
        );
        assert!(transient.expire(refreshed_at + HOST_CONTROL_HEADER_TIMEOUT));
        assert_eq!(
            transient.visible_message(refreshed_at + HOST_CONTROL_HEADER_TIMEOUT),
            None
        );
    }

    #[test]
    fn rapid_host_controls_keep_the_latest_value_of_each_target() {
        let events = [
            HostControlEvent {
                target: HostControlTarget::MasterLevel,
                value: 10,
            },
            HostControlEvent {
                target: HostControlTarget::MasterPan,
                value: 20,
            },
            HostControlEvent {
                target: HostControlTarget::MasterLevel,
                value: 30,
            },
        ];
        assert_eq!(
            coalesce_host_control_events(events),
            vec![
                HostControlEvent {
                    target: HostControlTarget::MasterPan,
                    value: 20,
                },
                HostControlEvent {
                    target: HostControlTarget::MasterLevel,
                    value: 30,
                },
            ]
        );
    }
}
