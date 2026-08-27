mod rackforge_parameters;

use rackforge_parameters::coalesce_rackforge_parameters;

use midir::{
    Ignore, MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection,
    MidiOutputPort,
};
#[cfg(target_os = "linux")]
use rackforge_control_api::CONTROL_SOCKET_NAME;
use rackforge_control_api::{
    ControlRequest, ControlResponse, VirtualMidiMessage, transport::ControlConnection,
};
use rackforge_controller_api::{ButtonPhase, HostActionBinding, HostActionTarget};
use rackforge_controller_api::{
    LITTLE_V1, rackforge_parameter_input, semantic_control_input, semantic_control_little_header,
};
use rackforge_controller_arturia_keylab_essential_mk3::{controller, protocol as keylab_protocol};
use rackforge_controller_package::{
    CONTROLLER_DRIVER_API_VERSION, PROCESS_DRIVER_PROTOCOL_VERSION, ProcessDriverInfo,
};
#[cfg(target_os = "linux")]
use rackforge_platform_api::{
    PLATFORM_CONTROL_SCHEMA_VERSION, PlatformControlPayload, PlatformControlRequest,
    PlatformControlResponse, PlatformOperation, WifiConnectionId, WifiPassphrase, WifiSsid,
};
use rackforge_session_api::SurfaceMode;
use rackforge_session_api::{
    ClientId, CommandEnvelope, EventEnvelope, InstanceId, PluginInstanceState,
    RackForgeParameterInput, RackForgeParameterMapper, RackForgeParameterValue,
    SemanticControlInput, SessionCommand, SessionState,
};
use rackforge_session_api::{SessionEvent, SurfaceActivationRequest};
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
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
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
const PART_CLEAR_HOLD_THRESHOLD: Duration = Duration::from_millis(1_500);
const HOME_CHORD_SIMULTANEITY: Duration = Duration::from_millis(250);
const HOST_CONTROL_HEADER_TIMEOUT: Duration = Duration::from_millis(1_500);
const SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(125);
#[cfg(target_os = "linux")]
const WEB_CONTROL_SOCKET_NAME: &str = "web-control.sock";
#[cfg(target_os = "linux")]
const PLATFORM_CONTROL_SOCKET: &str = "/run/rackforge/platform-control.sock";
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
    Monitor {
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
                    host_actions: profile.host_actions.clone(),
                    semantic_profile: profile.semantic_profile.clone(),
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
        Command::Monitor {
            selector,
            seconds,
            execute,
        } => {
            let selected = select_port(&ports, selector.as_deref())?;
            run_monitor(midi, selected, selector.as_deref(), seconds, execute)?;
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
        Some("demo" | "menu-demo" | "monitor" | "serve" | "restore" | "led-demo")
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
            "--seconds" if matches!(command, Some("demo" | "menu-demo" | "monitor")) => {
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
        Some("monitor") => Command::Monitor {
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
           rackforge-arturia-keylab-essential-mk3-driver monitor [--port ID|NOMBRE] [--seconds 1..120] [--execute]\n\
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
    _midi_forwarder: Option<MidiForwarder>,
    source_name: String,
    ack_receiver: Receiver<()>,
    input_receiver: Receiver<PhysicalInputEvent>,
    rackforge_parameter_receiver: Receiver<RackForgeParameterInput>,
    semantic_feedback_receiver: Receiver<SemanticControlInput>,
}

struct MidiForwarder {
    sender: Option<SyncSender<VirtualMidiMessage>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl MidiForwarder {
    fn from_environment(source_name: &str) -> Result<Option<Self>, String> {
        if env::var_os("RACKFORGE_FORWARD_MIDI").is_none() {
            return Ok(None);
        }
        let endpoint = rackforge_control_api::transport::endpoint_from_env(default_control_socket)
            .map_err(|error| format!("resolving MIDI forwarding endpoint: {error}"))?;
        let client_id = ClientId::new("controller.arturia.keylab-essential-mk3.midi")
            .map_err(|message| format!("invalid MIDI forwarding client id: {message}"))?;
        let source_name = source_name.to_owned();
        let (sender, receiver) = mpsc::sync_channel::<VirtualMidiMessage>(4096);
        let worker = thread::Builder::new()
            .name("rackforge-keylab-midi-forwarder".into())
            .spawn(move || {
                let mut connection = None;
                while let Ok(message) = receiver.recv() {
                    let request = ControlRequest::VirtualMidi {
                        client_id: client_id.clone(),
                        source_name: Some(source_name.clone()),
                        message,
                    };
                    let mut delivered = false;
                    for _ in 0..2 {
                        if connection.is_none() {
                            match ControlConnection::connect(&endpoint) {
                                Ok(opened) => connection = Some(opened),
                                Err(error) => {
                                    eprintln!("MIDI_FORWARD_CONNECT_FAILED error={error}");
                                    break;
                                }
                            }
                        }
                        match connection
                            .as_mut()
                            .expect("connection established")
                            .exchange(&request)
                        {
                            Ok(ControlResponse::VirtualMidiAccepted { .. }) => {
                                delivered = true;
                                break;
                            }
                            Ok(ControlResponse::Error { message, .. }) => {
                                eprintln!("MIDI_FORWARD_REJECTED error={message}");
                                break;
                            }
                            Ok(_) | Err(_) => connection = None,
                        }
                    }
                    if !delivered {
                        eprintln!("MIDI_FORWARD_DROPPED");
                    }
                }
                if let Some(mut connection) = connection {
                    let _ = connection.exchange(&ControlRequest::ReleaseVirtualMidi { client_id });
                }
            })
            .map_err(|error| format!("starting MIDI forwarding worker: {error}"))?;
        Ok(Some(Self {
            sender: Some(sender),
            worker: Some(worker),
        }))
    }

    fn sender(&self) -> Option<SyncSender<VirtualMidiMessage>> {
        self.sender.clone()
    }
}

impl Drop for MidiForwarder {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
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
    held: [Option<HeldButton>; 5],
    home_chord_emitted: bool,
}

impl ButtonGestureTracker {
    fn press(&mut self, input: menu::Input, now: Instant) -> bool {
        let Some(index) = gesture_button_index(input) else {
            return false;
        };
        self.held[index].get_or_insert(HeldButton {
            pressed_at: now,
            long_emitted: false,
        });
        true
    }

    fn release(&mut self, input: menu::Input, now: Instant) -> Option<menu::Input> {
        let index = gesture_button_index(input)?;
        let held = self.held[index].take()?;
        let gesture = if held.long_emitted {
            None
        } else if now.saturating_duration_since(held.pressed_at) >= gesture_threshold(input) {
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
            let input = [
                menu::Input::Button1,
                menu::Input::Button2,
                menu::Input::Button3,
                menu::Input::Button4,
                menu::Input::KeyboardParts,
            ][index];
            if !held.long_emitted
                && now.saturating_duration_since(held.pressed_at) >= gesture_threshold(input)
            {
                held.long_emitted = true;
                gestures.push(
                    input
                        .long_press()
                        .expect("tracked buttons have long gestures"),
                );
            }
        }
        gestures
    }

    fn consume(&mut self, input: menu::Input) -> bool {
        let Some(index) = gesture_button_index(input) else {
            return false;
        };
        let consumed = self.held[index].take().is_some();
        if self.held.iter().all(Option::is_none) {
            self.home_chord_emitted = false;
        }
        consumed
    }
}

fn gesture_threshold(input: menu::Input) -> Duration {
    if input == menu::Input::KeyboardParts {
        PART_CLEAR_HOLD_THRESHOLD
    } else {
        LONG_PRESS_THRESHOLD
    }
}

fn gesture_button_index(input: menu::Input) -> Option<usize> {
    match input {
        menu::Input::Button1 => Some(0),
        menu::Input::Button2 => Some(1),
        menu::Input::Button3 => Some(2),
        menu::Input::Button4 => Some(3),
        menu::Input::KeyboardParts => Some(4),
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

    fn send_messages(
        &mut self,
        messages: impl IntoIterator<Item = keylab_protocol::OutboundMessage>,
    ) -> Result<(), Box<dyn Error>> {
        for message in messages {
            self.send(&message.bytes)?;
            if message.settle_after_ms != 0 {
                thread::sleep(Duration::from_millis(u64::from(message.settle_after_ms)));
            }
        }
        Ok(())
    }

    fn start(&mut self) -> Result<(), Box<dyn Error>> {
        self.switched_to_daw = true;
        self.connected = true;
        self.send_messages(keylab_protocol::acquire_messages()?)
    }

    fn restore(&mut self) -> Result<(), Box<dyn Error>> {
        if self.connected || self.switched_to_daw {
            self.send_messages(keylab_protocol::restore_messages()?)?;
            self.connected = false;
            self.switched_to_daw = false;
        }
        Ok(())
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

fn run_monitor(
    midi: MidiOutput,
    port: &PortInfo,
    _input_selector: Option<&str>,
    seconds: u64,
    execute: bool,
) -> Result<(), Box<dyn Error>> {
    println!("Salida:  [{}] {}", port.index, port.name);
    if !execute {
        println!("DRY-RUN: use --execute para seleccionar el modo DAW y capturar MIDI.");
        return Ok(());
    }

    let mut session = KeyLabSession::open(midi, port)?;
    session.start()?;

    let input_names = {
        let mut discovery = MidiInput::new("rackforge KeyLab MIDI monitor discovery")?;
        discovery.ignore(Ignore::None);
        enumerate_input_ports(&discovery)?
            .into_iter()
            .filter(|input| {
                let folded = input.name.to_ascii_lowercase();
                (folded.contains("keylab") || folded.contains("kl essential"))
                    && !folded.contains("dinthru")
                    && !folded.contains("alv")
            })
            .map(|input| input.name)
            .collect::<Vec<_>>()
    };
    if input_names.is_empty() {
        return Err("No se encontraron entradas MIDI/MCU del KeyLab".into());
    }

    let (sender, receiver) = mpsc::sync_channel::<(String, Vec<u8>)>(4096);
    let mut connections = Vec::with_capacity(input_names.len());
    for input_name in input_names {
        let mut midi_input = MidiInput::new("rackforge KeyLab MIDI monitor")?;
        midi_input.ignore(Ignore::None);
        let input = enumerate_input_ports(&midi_input)?
            .into_iter()
            .find(|input| input.name == input_name)
            .ok_or_else(|| format!("La entrada MIDI {input_name:?} desapareció"))?;
        println!("Entrada: [{}] {}", input.index, input.name);
        let callback_name = input.name.clone();
        let callback_sender = sender.clone();
        connections.push(midi_input.connect(
            &input.handle,
            "rackforge KeyLab raw MIDI monitor",
            move |_timestamp, message, _context| {
                let _ = callback_sender.try_send((callback_name.clone(), message.to_vec()));
            },
            (),
        )?);
    }
    drop(sender);

    println!("MIDI_MONITOR_READY seconds={seconds}");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match receiver.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok((source, message)) if !message.is_empty() && message[0] != 0xf0 => {
                if message.len() >= 3 && message[0] & 0xf0 == 0xb0 {
                    println!(
                        "MIDI_CC source={source:?} channel={} controller={} value={} hex={}",
                        (message[0] & 0x0f) + 1,
                        message[1],
                        message[2],
                        hex(&message)
                    );
                } else {
                    println!("MIDI_RAW source={source:?} hex={}", hex(&message));
                }
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(connections);
    session.restore()?;
    println!("MIDI_MONITOR_DONE");
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

/// Where the host keeps this controller's user settings, if it told us.
fn controller_settings_path() -> Option<std::path::PathBuf> {
    env::var_os("RACKFORGE_CONTROLLER_SETTINGS").map(std::path::PathBuf::from)
}

/// Applies the settings file when its mtime moves. Returns true when a
/// value changed and the hardware should repaint. The only setting today
/// is `key-light-color` (#rrggbb, scaled to the SysEx 7-bit range).
fn refresh_controller_settings(last_modified: &mut Option<std::time::SystemTime>) -> bool {
    let Some(path) = controller_settings_path() else {
        return false;
    };
    let Ok(metadata) = std::fs::metadata(&path) else {
        return false;
    };
    let modified = metadata.modified().ok();
    if modified == *last_modified {
        return false;
    }
    *last_modified = modified;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        eprintln!("SETTINGS_INVALID path={}", path.display());
        return false;
    };
    let mut changed = false;
    if let Some(value) = table
        .get("key-light-color")
        .and_then(|value| value.as_str())
        && let Some(rgb) = parse_hex_color(value)
    {
        // The picker speaks 8-bit sRGB; the KeyLab's SysEx speaks 7-bit.
        keylab_protocol::set_ambient_led_rgb([rgb[0] >> 1, rgb[1] >> 1, rgb[2] >> 1]);
        println!("SETTINGS_APPLIED key-light-color={value}");
        changed = true;
    }
    changed
}

fn parse_hex_color(value: &str) -> Option<[u8; 3]> {
    let digits = value.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let parsed = u32::from_str_radix(digits, 16).ok()?;
    Some([
        ((parsed >> 16) & 0xFF) as u8,
        ((parsed >> 8) & 0xFF) as u8,
        (parsed & 0xFF) as u8,
    ])
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
    // A driver must never outlive its supervisor: when the host hands us a
    // piped stdin (RACKFORGE_SUPERVISOR_PIPE=1), EOF on it means the
    // supervisor died -- orphaned drivers were holding MIDI ports hostage.
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    if env::var_os("RACKFORGE_SUPERVISOR_PIPE").is_some() {
        let shutdown_requested = Arc::clone(&shutdown_requested);
        thread::spawn(move || {
            use std::io::Read;
            let mut byte = [0u8; 1];
            loop {
                match std::io::stdin().read(&mut byte) {
                    Ok(0) | Err(_) => {
                        eprintln!("Supervisor cerrado; restaurando el controlador...");
                        shutdown_requested.store(true, Ordering::Release);
                        break;
                    }
                    Ok(_) => {}
                }
            }
        });
    }
    let mut settings_modified: Option<std::time::SystemTime> = None;
    refresh_controller_settings(&mut settings_modified);
    println!("Esperando el KeyLab Essential mk3...");
    loop {
        if shutdown_requested.load(Ordering::Acquire) {
            return Ok(());
        }
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
        if let Err(error) = register_controller_bindings(None) {
            // A host that does not know the registration yet is not a host
            // to boycott: without the reservation the surface still works,
            // only the master-control CCs lack a formal claim.
            if error.contains("does not support") {
                eprintln!("El host no reserva bindings todavía; continuando sin reserva: {error}");
            } else {
                eprintln!("Core todavía no acepta el perfil del controlador: {error}");
                thread::sleep(Duration::from_millis(500));
                continue;
            }
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
        if let Err(error) = register_controller_bindings(Some(&input.source_name)) {
            eprintln!("Core no pudo asociar el perfil al endpoint MIDI: {error}");
            thread::sleep(Duration::from_millis(500));
            continue;
        }
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
        let mut next_settings_check = Instant::now();
        let mut parameter_mapper = RackForgeParameterMapper::default();
        'surface: loop {
            if shutdown_requested.load(Ordering::Acquire) {
                eprintln!("Restaurando OLED, LEDs y preset Arturia antes de salir...");
                return Ok(());
            }
            if control_socket_generation() != control_generation {
                eprintln!("Core cambió; cerrando MIDI para renovar los controles reservados...");
                break;
            }
            if usb_generation.is_some() && keylab_usb_generation() != usb_generation {
                eprintln!("Cambió la instancia USB del KeyLab; creando una sesión MIDI nueva...");
                break;
            }

            let parameter_events =
                coalesce_rackforge_parameters(input.rackforge_parameter_receiver.try_iter());
            let mut latest_feedback = None;
            for event in parameter_events {
                let current_pan = live_snapshot()
                    .map(|snapshot| snapshot.master_pan)
                    .unwrap_or_default();
                let Some(parameter) = parameter_mapper.apply(event, current_pan) else {
                    continue;
                };
                match apply_rackforge_parameter(parameter) {
                    Ok(()) => {
                        latest_feedback = Some(parameter.little_header());
                        println!(
                            "RACKFORGE_PARAMETER role={} value={}",
                            parameter.parameter().role(),
                            parameter.display_value()
                        );
                    }
                    Err(error) => eprintln!("No se pudo aplicar el parámetro global: {error}"),
                }
            }
            for feedback in input.semantic_feedback_receiver.try_iter() {
                latest_feedback = Some(semantic_control_little_header(&feedback));
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
                    if matches!(event.input, menu::Input::KeyboardSplitNote(_)) {
                        button_gestures.consume(menu::Input::KeyboardParts);
                    }
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
            } else if (wifi_task.is_some() || audio_task.is_some() || menu.is_plugin_loading())
                && now >= next_spinner_frame
            {
                next_spinner_frame = now + SPINNER_FRAME_INTERVAL;
                if menu.advance_wifi_spinner()
                    || menu.advance_audio_spinner()
                    || menu.advance_plugin_spinner()
                {
                    messages = render_menu_messages(&menu)?;
                    if let Err(error) = session.send(&messages.body) {
                        eprintln!("Could not advance async loader: {error}");
                        break;
                    }
                }
            }
            if transient_header.expire(now)
                && let Err(error) = session.send(&messages.header)
            {
                eprintln!("No se pudo restaurar el header del menú: {error}");
                break;
            }

            if now >= next_settings_check {
                next_settings_check = now + Duration::from_secs(1);
                if refresh_controller_settings(&mut settings_modified) {
                    if let Ok(repaint) = keylab_protocol::ambient_repaint_messages()
                        && let Err(error) = session.send_messages(repaint)
                    {
                        eprintln!("No se pudo repintar el color de teclas: {error}");
                        break 'surface;
                    }
                    // The footer buttons idle in the ambient too -- and
                    // their LED bytes are baked at render time, so re-render
                    // with the new ambient before sending.
                    messages = render_menu_messages(&menu)?;
                    for message in &messages.button_leds {
                        if let Err(error) = session.send(message) {
                            eprintln!("No se pudo refrescar los botones: {error}");
                            break 'surface;
                        }
                    }
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
                        let previous_button_leds = messages.button_leds.clone();
                        if let Err(error) = refresh_live_catalog(&mut menu) {
                            eprintln!("No se pudo refrescar el catálogo LIVE: {error}");
                        } else {
                            messages = render_menu_messages(&menu)?;
                            if let Err(error) = send_changed_button_leds(
                                &mut session,
                                &previous_button_leds,
                                &messages.button_leds,
                            ) {
                                eprintln!("No se pudo actualizar los botones: {error}");
                                break;
                            }
                        }
                    }
                    // A healthy OLED heartbeat only reasserts the display. Re-sending
                    // unchanged button LED messages makes the KeyLab briefly blank and
                    // restore the four soft-key LEDs every few seconds.
                    if let Err(error) = send_menu_display_with_header_override(
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
                .unwrap_or_else(|| {
                    env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("rackforge")
                });
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

fn control_request(request: &ControlRequest) -> Result<ControlResponse, String> {
    control_request_with_timeout(request, Duration::from_secs(1))
}

/// One control exchange through the shared transport: RACKFORGE_CONTROL_ADDR
/// (TCP loopback -- how a desktop or Android supervisor points this driver at
/// its core) wins over the platform's control socket.
fn control_request_with_timeout(
    request: &ControlRequest,
    _timeout: Duration,
) -> Result<ControlResponse, String> {
    let endpoint = rackforge_control_api::transport::endpoint_from_env(default_control_socket)
        .map_err(|error| error.to_string())?;
    rackforge_control_api::transport::exchange(&endpoint, request)
        .map_err(|error| error.to_string())
}

#[cfg(unix)]
fn default_control_socket() -> PathBuf {
    control_socket_path()
}

#[cfg(not(unix))]
fn default_control_socket() -> PathBuf {
    PathBuf::new()
}

fn live_snapshot() -> Result<SessionState, String> {
    match control_request(&ControlRequest::Snapshot)? {
        ControlResponse::Snapshot { snapshot } => Ok(*snapshot),
        ControlResponse::Error { message, .. } => Err(message),
        _ => Err("respuesta inesperada al pedir estado de sesión".into()),
    }
}

fn rackforge_presets(plugin_id: &str) -> Result<Vec<menu::PlayPreset>, String> {
    match control_request(&ControlRequest::PluginPresets {
        plugin_id: plugin_id.to_owned(),
    })? {
        ControlResponse::PluginPresets { presets, .. } => Ok(presets
            .into_iter()
            .map(|preset| {
                menu::PlayPreset::new(
                    preset.id,
                    preset.name,
                    format!("v{}", preset.plugin_version),
                )
            })
            .collect()),
        ControlResponse::Error { message, .. } => Err(message),
        _ => Err("unexpected response while listing RackForge presets".into()),
    }
}

fn active_plugin_instance(snapshot: &SessionState) -> Result<&PluginInstanceState, String> {
    snapshot
        .active_instance()
        .ok_or_else(|| "la sesión LIVE no tiene una instancia activa".to_owned())
}

fn play_plugins(snapshot: &SessionState) -> Vec<menu::PlayPlugin> {
    // LITTLE is a RackForge-owned surface. A plugin does not need to publish
    // its own `little@1` layout to participate: the host already owns the
    // plugin name, sound catalog and selection commands required by this
    // compact browser. `ui_layouts` only describes plugin-provided surfaces.
    snapshot
        .instances
        .iter()
        .map(|candidate| {
            menu::PlayPlugin::new(
                candidate.instance_id.as_str(),
                &candidate.plugin_id,
                &candidate.plugin_name,
            )
            .short_name(if candidate.plugin_short_name.is_empty() {
                &candidate.plugin_name
            } else {
                &candidate.plugin_short_name
            })
            .config_available(candidate.config_available)
        })
        .collect()
}

fn active_plugin_instance_id() -> Result<InstanceId, String> {
    let snapshot = live_snapshot()?;
    Ok(active_plugin_instance(&snapshot)?.instance_id.clone())
}

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

fn register_controller_bindings(midi_source_name: Option<&str>) -> Result<(), String> {
    let profile = controller::package_profile();
    dispatch_session_command(SessionCommand::RegisterHostBindings {
        controller_id: profile.driver_id.clone(),
        controls: profile.host_controls.clone(),
        actions: profile.host_actions.clone(),
        midi_source_name: midi_source_name.map(str::to_owned),
        semantic_profile: profile.semantic_profile.clone(),
    })?;
    println!(
        "HOST_BINDINGS_RESERVED controller={} controls={} actions={} semantic={}",
        profile.driver_id,
        profile.host_controls.len(),
        profile.host_actions.len(),
        profile
            .semantic_profile
            .as_ref()
            .map_or(0, |profile| profile.controls.len())
    );
    Ok(())
}

fn apply_rackforge_parameter(parameter: RackForgeParameterValue) -> Result<(), String> {
    match parameter {
        RackForgeParameterValue::MasterLevel(level) => {
            dispatch_session_command(SessionCommand::SetMasterLevel { level })?;
            Ok(())
        }
        RackForgeParameterValue::MasterPan(pan) => {
            dispatch_session_command(SessionCommand::SetMasterPan { pan })?;
            Ok(())
        }
    }
}

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
        SurfaceMode::Idle => menu::ActiveMode::Idle,
        SurfaceMode::Live => menu::ActiveMode::Live,
        SurfaceMode::Play => menu::ActiveMode::Play,
    });
    menu.set_play_plugins(
        play_plugins(&snapshot),
        snapshot.active_instance_id.as_ref().map(InstanceId::as_str),
    );
    let instance = active_plugin_instance(&snapshot)?;
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
    let sounds = instance
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
        .collect();
    if menu.sync_active_plugin(
        instance.instance_id.as_str(),
        &instance.plugin_id,
        &instance.plugin_name,
        sounds,
        selected.as_deref(),
    ) {
        match rackforge_presets(&instance.plugin_id) {
            Ok(presets) => menu.set_plugin_presets(presets, None),
            Err(error) => {
                menu.set_plugin_presets(Vec::new(), None);
                eprintln!("Could not refresh RackForge presets: {error}");
            }
        }
        menu.sync_program_edit(program_draft, audition_lease_id);
    }
    match control_request(&ControlRequest::PluginParameters {
        instance_id: instance.instance_id.clone(),
    }) {
        Ok(ControlResponse::PluginParameters { schema, values, .. }) => {
            menu.sync_plugin_parameters(
                *schema,
                values.into_iter().map(|value| (value.index, value.value)),
            );
        }
        Ok(ControlResponse::Error { message, .. }) | Err(message) => {
            eprintln!("Could not refresh LITTLE plugin parameters: {message}");
        }
        Ok(_) => eprintln!("Could not refresh LITTLE plugin parameters: unexpected response"),
    }
    Ok(())
}

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

#[cfg(not(target_os = "linux"))]
fn refresh_audio_settings(_menu: &mut menu::Menu) -> Result<(), String> {
    Ok(())
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

#[cfg(not(target_os = "linux"))]
fn refresh_wifi_settings(_menu: &mut menu::Menu) -> Result<(), String> {
    Ok(())
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

#[cfg(not(target_os = "linux"))]
fn refresh_web_settings(_menu: &mut menu::Menu) -> Result<(), String> {
    Ok(())
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
        // A string, not a flag: the host distinguishes a device with no PIN
        // that will still accept one from a device whose enrolment window has
        // closed. The display only needs to know whether one exists.
        pin_set: response
            .get("pin_state")
            .and_then(Value::as_str)
            .is_some_and(|state| state == "set"),
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn web_control_request(request: &Value) -> Result<Value, String> {
    let root = env::var_os("RACKFORGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("rackforge")
        });
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

fn keep_audition_alive(lease_id: u64) -> Result<(), String> {
    dispatch_session_command(SessionCommand::KeepAuditionAlive { lease_id }).map(|_| ())
}

// LITTLE navigation is portable controller behavior. Only the settings arms
// below are platform-specific; Play/Live navigation, sound selection, program
// editing and emergency stop must always reach the host control API.
fn apply_pending_menu_command(
    menu: &mut menu::Menu,
    wifi_task: &mut Option<WifiTask>,
    audio_task: &mut Option<AudioTask>,
) -> Result<bool, String> {
    #[cfg(not(target_os = "linux"))]
    let _ = (&mut *wifi_task, &mut *audio_task);
    let Some(command) = menu.take_command() else {
        return Ok(false);
    };
    match command {
        menu::MenuCommand::SetActiveMode { mode } => {
            dispatch_session_command(SessionCommand::SetActiveMode {
                mode: match mode {
                    menu::ActiveMode::Idle => SurfaceMode::Idle,
                    menu::ActiveMode::Live => SurfaceMode::Live,
                    menu::ActiveMode::Play => SurfaceMode::Play,
                },
            })?;
            println!("ACTIVE_MODE_SET mode={mode:?}");
            Ok(true)
        }
        menu::MenuCommand::SelectPlugin { instance_id } => {
            let instance_id = InstanceId::new(instance_id)
                .map_err(|message| format!("invalid plugin instance: {message}"))?;
            dispatch_session_command(SessionCommand::SelectPlugin {
                instance_id: instance_id.clone(),
            })?;
            refresh_live_catalog(menu)?;
            println!("PLUGIN_SELECTED instance={instance_id}");
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
        menu::MenuCommand::LoadPluginPreset { preset_id } => {
            let instance_id = active_plugin_instance_id()?;
            match control_request(&ControlRequest::LoadPluginPreset {
                instance_id,
                preset_id: preset_id.clone(),
            })? {
                ControlResponse::PluginPresetLoaded { .. } => {
                    menu.complete_plugin_preset_load(&preset_id);
                    refresh_live_catalog(menu)?;
                    println!("PLUGIN_PRESET_LOADED id={preset_id}");
                    Ok(true)
                }
                ControlResponse::Error { message, .. } => Err(message),
                _ => Err("unexpected response while loading RackForge preset".into()),
            }
        }
        menu::MenuCommand::SavePluginPreset { name } => {
            let response = active_plugin_instance_id().and_then(|instance_id| {
                control_request(&ControlRequest::SavePluginPreset {
                    instance_id,
                    name: name.clone(),
                })
            });
            match response {
                Ok(ControlResponse::PluginPresetSaved { preset, presets }) => {
                    let saved = menu::PlayPreset::new(
                        preset.id.clone(),
                        preset.name.clone(),
                        format!("v{}", preset.state.plugin_version),
                    );
                    let presets = presets
                        .into_iter()
                        .map(|preset| {
                            menu::PlayPreset::new(
                                preset.id,
                                preset.name,
                                format!("v{}", preset.plugin_version),
                            )
                        })
                        .collect();
                    menu.complete_plugin_preset_save(Ok((saved, presets)));
                    println!("PLUGIN_PRESET_SAVED name={}", preset.name);
                    Ok(true)
                }
                Ok(ControlResponse::Error { message, .. }) | Err(message) => {
                    menu.complete_plugin_preset_save(Err(message.clone()));
                    println!("PLUGIN_PRESET_SAVE_FAILED message={message}");
                    Ok(true)
                }
                Ok(_) => {
                    menu.complete_plugin_preset_save(Err(
                        "unexpected response while saving RackForge preset".into(),
                    ));
                    Ok(true)
                }
            }
        }
        menu::MenuCommand::SetPluginParameter {
            instance_id,
            parameter_index,
            value,
        } => {
            let instance_id = InstanceId::new(instance_id)
                .map_err(|message| format!("invalid plugin instance: {message}"))?;
            match control_request(&ControlRequest::SetPluginParameter {
                instance_id,
                parameter_index,
                value,
            })? {
                ControlResponse::PluginParameterSet { value, .. } => {
                    menu.complete_plugin_parameter_set(parameter_index, value);
                    Ok(true)
                }
                ControlResponse::Error { message, .. } => Err(message),
                _ => Err("unexpected response while setting a plugin parameter".into()),
            }
        }
        menu::MenuCommand::TriggerPluginParameter {
            instance_id,
            parameter_index,
        } => {
            let instance_id = InstanceId::new(instance_id)
                .map_err(|message| format!("invalid plugin instance: {message}"))?;
            for value in [1.0, 0.0] {
                match control_request(&ControlRequest::SetPluginParameter {
                    instance_id: instance_id.clone(),
                    parameter_index,
                    value,
                })? {
                    ControlResponse::PluginParameterSet { value, .. } => {
                        menu.complete_plugin_parameter_set(parameter_index, value);
                    }
                    ControlResponse::Error { message, .. } => return Err(message),
                    _ => {
                        return Err(
                            "unexpected response while triggering a plugin parameter".into()
                        );
                    }
                }
            }
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
        #[cfg(target_os = "linux")]
        menu::MenuCommand::SetWebEnabled { enabled } => {
            web_control_request(&serde_json::json!({
                "op": "set",
                "field": "enabled",
                "value": enabled
            }))?;
            println!("WEB_SETTING_SET field=enabled value={enabled}");
            Ok(true)
        }
        #[cfg(target_os = "linux")]
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
        #[cfg(target_os = "linux")]
        menu::MenuCommand::SetWebPort { port } => {
            web_control_request(&serde_json::json!({
                "op": "set",
                "field": "port",
                "value": port
            }))?;
            println!("WEB_SETTING_SET field=port value={port}");
            Ok(true)
        }
        #[cfg(target_os = "linux")]
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
        #[cfg(target_os = "linux")]
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
        #[cfg(target_os = "linux")]
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
        #[cfg(target_os = "linux")]
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
        #[cfg(target_os = "linux")]
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
        #[cfg(target_os = "linux")]
        menu::MenuCommand::ScanWifi => {
            start_wifi_scan(wifi_task, menu)?;
            Ok(true)
        }
        #[cfg(target_os = "linux")]
        menu::MenuCommand::ApplyAudioOutput { profile } => {
            start_audio_change(audio_task, menu, profile)?;
            Ok(true)
        }
        menu::MenuCommand::ForceHome => {
            dispatch_session_command(SessionCommand::EmergencyStop)?;
            println!("HOME_FORCED audio=stopped mode=idle");
            Ok(true)
        }
        #[cfg(not(target_os = "linux"))]
        menu::MenuCommand::SetWebEnabled { .. }
        | menu::MenuCommand::SetWebAccess { .. }
        | menu::MenuCommand::SetWebPort { .. }
        | menu::MenuCommand::ActivateSavedWifi { .. }
        | menu::MenuCommand::ForgetSavedWifi { .. }
        | menu::MenuCommand::ConnectDiscoveredWifi { .. }
        | menu::MenuCommand::DisconnectWifi
        | menu::MenuCommand::SetWifiEnabled { .. }
        | menu::MenuCommand::ScanWifi
        | menu::MenuCommand::ApplyAudioOutput { .. } => {
            Err("this controller setting is not available on the current platform".into())
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

fn return_to_active_mode(
    menu: &mut menu::Menu,
    mode: menu::ActiveMode,
    selected_sound_id: Option<String>,
) -> Result<(), String> {
    let surface_mode = match mode {
        menu::ActiveMode::Idle => {
            menu.complete_return_to_active_mode(mode, None);
            return Ok(());
        }
        menu::ActiveMode::Live => SurfaceMode::Live,
        menu::ActiveMode::Play => SurfaceMode::Play,
    };
    let instance_id = active_plugin_instance_id()?;
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
    let (rackforge_parameter_sender, rackforge_parameter_receiver) = mpsc::channel();
    let (semantic_feedback_sender, semantic_feedback_receiver) = mpsc::channel();
    let semantic_profile = controller::package_profile().semantic_profile.clone();
    let host_actions = controller::package_profile().host_actions.clone();
    let midi_forwarder = MidiForwarder::from_environment(&port.name)?;
    let midi_forward_sender = midi_forwarder.as_ref().and_then(MidiForwarder::sender);
    let mut keyboard_parts_held = false;
    let mut split_note_sent = false;
    let connection = midi.connect(
        &port.handle,
        "rackforge KeyLab DAW ACK",
        move |_timestamp, message, _context| {
            let mut consumed_by_surface = false;
            if is_daw_preset_ack(message) {
                let _ = ack_sender.send(());
                consumed_by_surface = true;
            }
            if let Some(input) = parse_physical_input(message) {
                let _ = input_sender.send(input);
                consumed_by_surface = true;
            }
            if let Some(input) = parse_host_action(message, &host_actions) {
                if input.input == menu::Input::KeyboardParts {
                    keyboard_parts_held = input.phase == InputPhase::Press;
                    if keyboard_parts_held {
                        split_note_sent = false;
                    }
                }
                let _ = input_sender.send(input);
                consumed_by_surface = true;
            }
            if keyboard_parts_held
                && !split_note_sent
                && let Some(note) = parse_split_note(message)
            {
                split_note_sent = true;
                let _ = input_sender.send(input_event(
                    menu::Input::KeyboardSplitNote(note),
                    InputPhase::Turn,
                ));
            }
            if let Some(input) = semantic_profile
                .as_ref()
                .and_then(|profile| semantic_control_input(profile, message))
            {
                if let Some(parameter) = semantic_profile
                    .as_ref()
                    .and_then(|profile| rackforge_parameter_input(profile, message))
                {
                    let _ = rackforge_parameter_sender.send(parameter);
                    consumed_by_surface = true;
                } else {
                    let _ = semantic_feedback_sender.send(input);
                }
            }
            if let Some(message) = forwardable_performance_message(message, consumed_by_surface)
                && let Some(sender) = midi_forward_sender.as_ref()
                && sender.try_send(message).is_err()
            {
                eprintln!("MIDI_FORWARD_QUEUE_FULL");
            }
        },
        (),
    )?;
    Ok(KeyLabInput {
        _connection: connection,
        _midi_forwarder: midi_forwarder,
        source_name: port.name.clone(),
        ack_receiver,
        input_receiver,
        rackforge_parameter_receiver,
        semantic_feedback_receiver,
    })
}

fn forwardable_performance_message(
    message: &[u8],
    consumed_by_surface: bool,
) -> Option<VirtualMidiMessage> {
    if consumed_by_surface || message.len() != 3 || message[0] & 0xf0 == 0xf0 {
        return None;
    }
    Some(VirtualMidiMessage {
        status: message[0],
        data1: message[1],
        data2: message[2],
    })
}

fn parse_split_note(message: &[u8]) -> Option<u8> {
    if message.len() == 3 && message[0] & 0xf0 == 0x90 && message[2] > 0 {
        Some(message[1])
    } else {
        None
    }
}

fn parse_host_action(message: &[u8], bindings: &[HostActionBinding]) -> Option<PhysicalInputEvent> {
    bindings.iter().find_map(|binding| {
        let input = match binding.target {
            HostActionTarget::KeyboardParts => menu::Input::KeyboardParts,
        };
        let phase = match binding.midi_cc.phase(message)? {
            ButtonPhase::Press => InputPhase::Press,
            ButtonPhase::Release => InputPhase::Release,
        };
        Some(input_event(input, phase))
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
    button_leds: [Vec<u8>; 4],
}

fn render_menu_messages(menu: &menu::Menu) -> Result<MenuMessages, String> {
    render_screen_messages(&menu.render())
}

fn render_screen_messages(screen: &menu::Screen) -> Result<MenuMessages, String> {
    Ok(MenuMessages {
        header: screen_header_message(&screen.header)?,
        body: two_lines(&screen.line_1, &screen.line_2)?,
        footer: footer(&screen.footer)?,
        button_leds: keylab_protocol::button_led_messages(&screen.footer)?,
    })
}

fn screen_header_message(header_mode: &menu::Header) -> Result<Vec<u8>, String> {
    match header_mode.text(menu::DISPLAY_COLUMNS) {
        Some(title) => header(&title),
        None => header(" "),
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
    send_menu_display_with_header_override(session, messages, header_override)?;
    for message in &messages.button_leds {
        session.send(message)?;
    }
    Ok(())
}

fn send_menu_display_with_header_override(
    session: &mut KeyLabSession,
    messages: &MenuMessages,
    header_override: Option<&[u8]>,
) -> Result<(), Box<dyn Error>> {
    session.send(&messages.body)?;
    thread::sleep(Duration::from_millis(20));
    session.send(header_override.unwrap_or(&messages.header))?;
    thread::sleep(Duration::from_millis(20));
    session.send(&messages.footer)?;
    Ok(())
}

fn send_changed_button_leds(
    session: &mut KeyLabSession,
    previous: &[Vec<u8>; 4],
    current: &[Vec<u8>; 4],
) -> Result<(), Box<dyn Error>> {
    for index in changed_button_led_indices(previous, current) {
        session.send(&current[index])?;
    }
    Ok(())
}

fn changed_button_led_indices(previous: &[Vec<u8>; 4], current: &[Vec<u8>; 4]) -> Vec<usize> {
    previous
        .iter()
        .zip(current)
        .enumerate()
        .filter_map(|(index, (previous, current))| (previous != current).then_some(index))
        .collect()
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

#[cfg(any(target_os = "linux", test))]
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
        println!(
            "DRY-RUN: se probarían temporalmente los cuatro LED RGB y luego quedarían apagados."
        );
        return Ok(());
    }
    if !is_keylab_midi(&port.name) {
        return Err("El puerto seleccionado no es el MIDI principal del KeyLab".into());
    }

    let mut session = KeyLabSession::open(midi, port)?;
    let result = (|| -> Result<(), Box<dyn Error>> {
        session.start()?;
        for message in keylab_protocol::clear_button_led_messages() {
            session.send(&message)?;
        }
        for index in 0..4 {
            session.send(&keylab_protocol::button_led_message(
                index,
                [127, 127, 127],
            )?)?;
            thread::sleep(Duration::from_millis(700));
            session.send(&keylab_protocol::button_led_message(index, [0, 0, 0])?)?;
            thread::sleep(Duration::from_millis(150));
        }
        for index in 0..4 {
            session.send(&keylab_protocol::button_led_message(index, [20, 80, 127])?)?;
        }
        thread::sleep(Duration::from_secs(1));
        Ok(())
    })();
    for (index, message) in keylab_protocol::clear_button_led_messages()
        .into_iter()
        .enumerate()
    {
        if let Err(error) = session.send(&message) {
            eprintln!("No se pudo apagar el LED {}: {error}", index + 1);
        }
    }
    if let Err(error) = session.restore() {
        eprintln!("No se pudo restaurar el modo Arturia después de la prueba: {error}");
    }
    result?;
    println!("Prueba LED RGB finalizada; los cuatro quedaron apagados.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_instance(
        instance_id: &str,
        plugin_id: &str,
        name: &str,
        ui_layouts: &[&str],
    ) -> PluginInstanceState {
        PluginInstanceState {
            instance_id: InstanceId::new(instance_id).unwrap(),
            plugin_id: plugin_id.into(),
            plugin_name: name.into(),
            plugin_short_name: name.into(),
            ui_layouts: ui_layouts.iter().map(|layout| (*layout).into()).collect(),
            config_available: false,
            banks: Vec::new(),
            sounds: Vec::new(),
            selected_sound_id: None,
        }
    }

    #[test]
    fn little_play_catalog_includes_host_rendered_plugins_without_a_little_layout() {
        let piano_id = InstanceId::new("play.piano").unwrap();
        let mut snapshot = SessionState::new(
            rackforge_session_api::SessionId::new("test.little-catalog").unwrap(),
        );
        snapshot.instances = vec![
            plugin_instance(
                piano_id.as_str(),
                "org.rackforge.concert-grand",
                "Concert Grand",
                &[],
            ),
            plugin_instance(
                "play.rf-106",
                "org.rackforge.rf-106",
                "RF-106",
                &[LITTLE_V1],
            ),
        ];
        snapshot.active_instance_id = Some(piano_id);

        let catalog = play_plugins(&snapshot);
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].plugin_id, "org.rackforge.concert-grand");
        assert_eq!(catalog[1].plugin_id, "org.rackforge.rf-106");
        assert_eq!(
            active_plugin_instance(&snapshot).unwrap().plugin_id,
            "org.rackforge.concert-grand"
        );
    }

    #[test]
    fn forwards_performance_midi_but_not_controller_surface_messages() {
        assert_eq!(
            forwardable_performance_message(&[0x90, 60, 100], false),
            Some(VirtualMidiMessage {
                status: 0x90,
                data1: 60,
                data2: 100,
            })
        );
        assert!(forwardable_performance_message(&[0x80, 60, 0], false).is_some());
        assert!(forwardable_performance_message(&[0xb0, 64, 127], false).is_some());
        // Ordinary faders remain visible to Desktop MIDI Learn and Parameter
        // Links even though the .rfcontroller owns the physical endpoint.
        assert!(forwardable_performance_message(&[0xb0, 82, 96], false).is_some());
        assert!(forwardable_performance_message(&[0xe0, 0, 64], false).is_some());
        // The four LITTLE soft keys are an intentional controller-plane
        // reservation. They navigate RackForge and never become musical MIDI.
        for controller in 44..=47 {
            let message = [0xb0, controller, 127];
            assert!(parse_physical_input(&message).is_some());
            assert!(forwardable_performance_message(&message, true).is_none());
        }
        assert!(forwardable_performance_message(&[0xb0, 113, 127], true).is_none());
        assert!(forwardable_performance_message(&[0xc0, 4], false).is_none());
        assert!(forwardable_performance_message(&[0xf8], false).is_none());
        assert!(forwardable_performance_message(&[0xf0, 0x7d, 0xf7], false).is_none());
    }

    #[test]
    fn only_changed_button_leds_are_selected_for_refresh() {
        let previous = [vec![0], vec![1], vec![2], vec![3]];
        assert!(changed_button_led_indices(&previous, &previous).is_empty());

        let current = [vec![0], vec![9], vec![2], vec![8]];
        assert_eq!(changed_button_led_indices(&previous, &current), vec![1, 3]);
    }

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
    fn long_back_is_emitted_while_held_and_not_repeated_on_release() {
        let started = Instant::now();
        let mut tracker = ButtonGestureTracker::default();
        assert!(tracker.press(menu::Input::Button4, started));
        assert_eq!(
            tracker.poll(started + LONG_PRESS_THRESHOLD),
            vec![menu::Input::Button4Long]
        );
        assert!(
            tracker
                .release(
                    menu::Input::Button4,
                    started + LONG_PRESS_THRESHOLD + Duration::from_millis(1)
                )
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
    fn part_button_is_a_declared_momentary_host_action() {
        let bindings = &controller::package_profile().host_actions;
        assert_eq!(
            parse_host_action(&[0xb0, 119, 127], bindings),
            Some(input_event(menu::Input::KeyboardParts, InputPhase::Press))
        );
        assert_eq!(
            parse_host_action(&[0xb0, 119, 0], bindings),
            Some(input_event(menu::Input::KeyboardParts, InputPhase::Release))
        );
        assert_eq!(parse_host_action(&[0xb0, 119, 64], bindings), None);
    }

    #[test]
    fn part_button_supports_short_and_long_gestures() {
        let started = Instant::now();
        let mut tracker = ButtonGestureTracker::default();
        assert!(tracker.press(menu::Input::KeyboardParts, started));
        assert_eq!(
            tracker.release(
                menu::Input::KeyboardParts,
                started + Duration::from_millis(100)
            ),
            Some(menu::Input::KeyboardParts)
        );
        assert!(tracker.press(menu::Input::KeyboardParts, started));
        assert!(tracker.poll(started + LONG_PRESS_THRESHOLD).is_empty());
        assert_eq!(
            tracker.poll(started + PART_CLEAR_HOLD_THRESHOLD),
            vec![menu::Input::KeyboardPartsLong]
        );
    }

    #[test]
    fn part_note_consumes_the_pending_button_gesture() {
        let started = Instant::now();
        let mut tracker = ButtonGestureTracker::default();
        assert!(tracker.press(menu::Input::KeyboardParts, started));
        assert_eq!(parse_split_note(&[0x92, 64, 100]), Some(64));
        assert_eq!(parse_split_note(&[0x92, 64, 0]), None);
        assert_eq!(parse_split_note(&[0x82, 64, 0]), None);
        assert!(tracker.consume(menu::Input::KeyboardParts));
        assert_eq!(
            tracker.release(
                menu::Input::KeyboardParts,
                started + PART_CLEAR_HOLD_THRESHOLD
            ),
            None
        );
        assert!(tracker.poll(started + PART_CLEAR_HOLD_THRESHOLD).is_empty());
    }

    #[test]
    fn semantic_global_parameter_is_separate_from_surface_navigation() {
        let profile = controller::package_profile()
            .semantic_profile
            .as_ref()
            .unwrap();
        let input = rackforge_parameter_input(profile, &[0xb0, 113, 91]).unwrap();
        assert_eq!(
            input.parameter,
            rackforge_session_api::RackForgeParameterId::MasterLevel
        );
        assert_eq!(input.value, 91);
        assert!(rackforge_parameter_input(profile, &[0xb0, 83, 91]).is_none());
        assert_eq!(parse_physical_input(&[0xb0, 113, 91]), None);
    }

    #[test]
    fn global_parameter_feedback_fits_the_native_header() {
        use rackforge_session_api::{MasterLevel, MasterPan};
        assert_eq!(
            RackForgeParameterValue::MasterLevel(MasterLevel::SILENT).little_header(),
            "MASTER VOL      0%"
        );
        assert_eq!(
            RackForgeParameterValue::MasterLevel(MasterLevel::UNITY).little_header(),
            "MASTER VOL    100%"
        );
        assert_eq!(
            RackForgeParameterValue::MasterPan(MasterPan::CENTER).little_header(),
            "MASTER PAN  CENTER"
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
    fn rapid_global_parameters_keep_the_latest_value_of_each_target() {
        use rackforge_session_api::{RackForgeParameterId, SemanticControlMode};
        let events = [
            RackForgeParameterInput {
                parameter: RackForgeParameterId::MasterLevel,
                value: 10,
                mode: SemanticControlMode::Absolute,
            },
            RackForgeParameterInput {
                parameter: RackForgeParameterId::MasterPan,
                value: 20,
                mode: SemanticControlMode::Relative,
            },
            RackForgeParameterInput {
                parameter: RackForgeParameterId::MasterLevel,
                value: 30,
                mode: SemanticControlMode::Absolute,
            },
        ];
        assert_eq!(
            coalesce_rackforge_parameters(events),
            vec![
                RackForgeParameterInput {
                    parameter: RackForgeParameterId::MasterPan,
                    value: 20,
                    mode: SemanticControlMode::Relative,
                },
                RackForgeParameterInput {
                    parameter: RackForgeParameterId::MasterLevel,
                    value: 30,
                    mode: SemanticControlMode::Absolute,
                },
            ]
        );
    }
}
