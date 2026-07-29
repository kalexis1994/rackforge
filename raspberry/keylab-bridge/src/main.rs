use midir::{MidiOutput, MidiOutputConnection, MidiOutputPort};
use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::thread;
use std::time::Duration;

const PREFIX: &[u8] = &[0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42];
const CONNECT: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x02, 0x0F, 0x40, 0x5A, 0x01, 0xF7,
];
const DISCONNECT: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x02, 0x0F, 0x40, 0x5A, 0x00, 0xF7,
];
const CLEAR_SCREEN: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x60, 0x61, 0xF7,
];

#[derive(Debug)]
struct Cli {
    command: Command,
}

#[derive(Debug)]
enum Command {
    List,
    Demo {
        selector: Option<String>,
        seconds: u64,
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
    let midi = MidiOutput::new("artupy KeyLab Bridge")?;
    let ports = enumerate_ports(&midi)?;

    match cli.command {
        Command::List => print_ports(&ports),
        Command::Demo {
            selector,
            seconds,
            execute,
        } => {
            let selected = select_port(&ports, selector.as_deref())?;
            run_demo(midi, selected, seconds, execute)?;
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
    if args.first().map(String::as_str) != Some("demo") {
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
            "--seconds" => {
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
    Ok(Cli {
        command: Command::Demo {
            selector,
            seconds,
            execute,
        },
    })
}

fn usage(reason: &str) -> String {
    format!(
        "{reason}\n\
         Uso:\n\
           artupy-bridge list\n\
           artupy-bridge demo [--port ID|NOMBRE] [--seconds 1..120] [--execute]"
    )
}

#[derive(Clone)]
struct PortInfo {
    index: usize,
    name: String,
    handle: MidiOutputPort,
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

fn is_keylab_midi(name: &str) -> bool {
    let trimmed = name.trim();
    let endpoint = trimmed
        .rsplit_once(' ')
        .filter(|(_, suffix)| is_alsa_address(suffix))
        .map_or(trimmed, |(prefix, _)| prefix);
    let folded = endpoint.to_ascii_lowercase();
    (folded.contains("kl essential") || folded.contains("keylab"))
        && folded.trim_end().ends_with("midi")
        && !folded.contains("mcu")
        && !folded.contains("hui")
        && !folded.contains("dinthru")
        && !folded.contains(" alv")
}

fn is_alsa_address(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(client, port)| {
            !client.is_empty()
                && !port.is_empty()
                && client.bytes().all(|byte| byte.is_ascii_digit())
                && port.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn print_ports(ports: &[PortInfo]) {
    println!("Puertos MIDI de salida:");
    for port in ports {
        let marker = if is_keylab_midi(&port.name) {
            "  <KeyLab recomendado>"
        } else {
            ""
        };
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
            .filter(|port| is_keylab_midi(&port.name))
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

struct KeyLabSession {
    connection: MidiOutputConnection,
    switched_to_daw: bool,
    connected: bool,
}

impl KeyLabSession {
    fn open(midi: MidiOutput, port: &PortInfo) -> Result<Self, Box<dyn Error>> {
        if !is_keylab_midi(&port.name) {
            return Err(format!(
                "El puerto [{}] {} no es el endpoint MIDI seguro del KeyLab",
                port.index, port.name
            )
            .into());
        }
        let connection = midi.connect(&port.handle, "artupy KeyLab SysEx")?;
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
    let title = header("ARTUPY")?;
    let screen = two_lines("ARTUPY", "PI CONNECTED")?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn recognizes_only_numeric_alsa_suffixes() {
        assert!(is_alsa_address("28:0"));
        assert!(!is_alsa_address("MIDI"));
        assert!(!is_alsa_address("28:MIDI"));
    }

    #[test]
    fn rejects_unsafe_screen_text() {
        assert!(two_lines("DOOM", "Ñ").is_err());
        assert!(two_lines("DOOM", "X".repeat(19).as_str()).is_err());
    }
}
