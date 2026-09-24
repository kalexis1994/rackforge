//! Reads what every KeyLab control sends, one control at a time, and writes
//! the map down: the companion of `led_sweep`, for input instead of light.
//!
//! The manuals say what a control *can* be set to send, not what this
//! unit's program sends; so, as with the LEDs, the device is asked. Each
//! control is named in turn, the first message it sends and everything
//! within half a second after it (the release, a pad's pressure) are shown,
//! and `MIDI-MAP.md` gathers them all at the end.
//!
//! `cargo run -p rackforge-controller-arturia-keylab-essential-mk3 --example midi_map`
//! `... --example midi_map -- --monitor` just prints every message.
use midir::{Ignore, MidiInput, MidiInputConnection};
use rackforge_controller_arturia_keylab_essential_mk3::controller;
use std::fmt::Write as _;
use std::io::{BufRead, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// The controls to read, in the order they are asked for.
const CONTROLS: &[&str] = &[
    "Pad 1 (banco A)",
    "Pad 2 (banco A)",
    "Pad 3 (banco A)",
    "Pad 4 (banco A)",
    "Pad 5 (banco A)",
    "Pad 6 (banco A)",
    "Pad 7 (banco A)",
    "Pad 8 (banco A)",
    "Pad 1 (banco B)",
    "Pad 2 (banco B)",
    "Pad 3 (banco B)",
    "Pad 4 (banco B)",
    "Pad 5 (banco B)",
    "Pad 6 (banco B)",
    "Pad 7 (banco B)",
    "Pad 8 (banco B)",
    "MIDI Ch",
    "Bank",
    "Transp -",
    "Transp +",
    "Oct -",
    "Oct +",
    "Prog",
    "Part",
    "Arp",
    "Chord",
    "Scale",
    "Hold",
    "Save",
    "Quant",
    "Undo",
    "Redo",
    "Loop",
    "<< (rebobinar)",
    ">> (avanzar)",
    "Metronome",
    "Stop",
    "Play",
    "Record",
    "TAP",
    "Boton OLED 1",
    "Boton OLED 2",
    "Boton OLED 3",
    "Boton OLED 4",
    "Encoder principal (girar)",
    "Encoder principal (apretar)",
    "Pitch bend",
    "Modulacion",
    "Pedal de sustain (si hay)",
];

/// How long after a control's first message its other messages still count.
const CAPTURE_WINDOW: Duration = Duration::from_millis(500);

struct Heard {
    port: String,
    bytes: Vec<u8>,
}

enum Event {
    Midi(Heard),
    Line(String),
}

fn describe(bytes: &[u8]) -> String {
    let Some(&status) = bytes.first() else {
        return "(vacio)".into();
    };
    if status == 0xf0 {
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        return format!("SysEx {hex}");
    }
    let channel = (status & 0x0f) + 1;
    let data = |index: usize| bytes.get(index).copied().unwrap_or(0);
    match status & 0xf0 {
        0x80 => format!(
            "Note off  canal {channel:>2}  nota {:>3}  vel {:>3}",
            data(1),
            data(2)
        ),
        0x90 if data(2) == 0 => {
            format!("Note off  canal {channel:>2}  nota {:>3}  (vel 0)", data(1))
        }
        0x90 => format!(
            "Note on   canal {channel:>2}  nota {:>3}  vel {:>3}",
            data(1),
            data(2)
        ),
        0xa0 => format!(
            "Poly AT   canal {channel:>2}  nota {:>3}  valor {:>3}",
            data(1),
            data(2)
        ),
        0xb0 => format!(
            "CC        canal {channel:>2}  cc {:>3}    valor {:>3}",
            data(1),
            data(2)
        ),
        0xc0 => format!("Program   canal {channel:>2}  programa {:>3}", data(1)),
        0xd0 => format!("Chan AT   canal {channel:>2}  valor {:>3}", data(1)),
        0xe0 => {
            let value = i32::from(data(1)) | (i32::from(data(2)) << 7);
            format!("PitchBend canal {channel:>2}  valor {:>5}", value - 8192)
        }
        _ => format!(
            "Sistema   {}",
            bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}

/// Clock and active sensing arrive on their own, from no control.
fn is_background(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(0xf8 | 0xfe))
}

fn open_inputs(
    sender: &mpsc::Sender<Event>,
    everything: bool,
) -> Result<Vec<MidiInputConnection<()>>, Box<dyn std::error::Error>> {
    let probe = MidiInput::new("rackforge-midi-map")?;
    let mut names = Vec::new();
    for port in probe.ports() {
        names.push(probe.port_name(&port)?);
    }
    let keylab: Vec<_> = names
        .iter()
        .filter(|name| controller::is_keylab_endpoint(name))
        .cloned()
        .collect();
    let chosen = if keylab.is_empty() || everything {
        names.clone()
    } else {
        keylab
    };
    if chosen.is_empty() {
        return Err("no hay ninguna entrada MIDI; conecta el KeyLab".into());
    }
    let mut connections = Vec::new();
    for name in chosen {
        let mut input = MidiInput::new("rackforge-midi-map")?;
        input.ignore(Ignore::None);
        let Some(port) = input.ports().into_iter().find(|port| {
            input
                .port_name(port)
                .is_ok_and(|candidate| candidate == name)
        }) else {
            continue;
        };
        let sender = sender.clone();
        let label = name.clone();
        match input.connect(
            &port,
            "rackforge-midi-map",
            move |_, bytes, _| {
                if !is_background(bytes) {
                    let _ = sender.send(Event::Midi(Heard {
                        port: label.clone(),
                        bytes: bytes.to_vec(),
                    }));
                }
            },
            (),
        ) {
            Ok(connection) => {
                println!("  escuchando: {name}");
                connections.push(connection);
            }
            Err(error) => println!("  no se pudo abrir {name}: {error}"),
        }
    }
    Ok(connections)
}

fn spawn_stdin(sender: mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if sender.send(Event::Line(line)).is_err() {
                break;
            }
        }
    });
}

fn monitor(receiver: &Receiver<Event>) {
    println!("\nModo monitor: toca cualquier control. Ctrl+C para salir.\n");
    for event in receiver.iter() {
        if let Event::Midi(heard) = event {
            println!("{:<42} {}", heard.port, describe(&heard.bytes));
        }
    }
}

/// Everything that arrives until `window` passes with the first message.
fn capture(receiver: &Receiver<Event>, first: Heard) -> Vec<Heard> {
    let deadline = Instant::now() + CAPTURE_WINDOW;
    let mut heard = vec![first];
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match receiver.recv_timeout(left) {
            Ok(Event::Midi(more)) => heard.push(more),
            Ok(Event::Line(_)) => {}
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    heard
}

/// A pad's pressure can send dozens of messages: keep each distinct kind
/// once, with the first and last value seen.
fn summarize(heard: &[Heard]) -> Vec<String> {
    let mut lines: Vec<(String, String, u8, u8)> = Vec::new();
    for message in heard {
        let status = message.bytes.first().copied().unwrap_or(0);
        let kind = describe(&message.bytes);
        let key_len = kind.len().min(26);
        let key = format!("{}|{}", message.port, &kind[..key_len]);
        let value = message.bytes.last().copied().unwrap_or(0);
        match lines.iter_mut().find(|(existing, ..)| *existing == key) {
            Some((_, _, _, last)) if status & 0xf0 != 0x90 && status & 0xf0 != 0x80 => {
                *last = value;
            }
            Some(_) => {}
            None => lines.push((key, format!("{}  {kind}", message.port), value, value)),
        }
    }
    lines
        .into_iter()
        .map(|(_, text, first, last)| {
            if first != last {
                format!("{text}   (recorrio {first}..{last})")
            } else {
                text
            }
        })
        .collect()
}

/// Opened with a double click, the window would close on an error before it
/// could be read.
fn main() {
    if let Err(error) = run() {
        println!("\nError: {error}");
        println!("Pulsa Enter para cerrar.");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    let monitor_only = arguments.iter().any(|argument| argument == "--monitor");
    let everything = arguments.iter().any(|argument| argument == "--todas");
    println!("RackForge - mapa MIDI del KeyLab Essential mk3");
    println!("Entradas:");
    let (sender, receiver) = mpsc::channel();
    let _connections = open_inputs(&sender, everything)?;
    spawn_stdin(sender);
    if monitor_only {
        monitor(&receiver);
        return Ok(());
    }
    println!(
        "\nPara cada control: tocalo una vez (apretar y soltar).\n\
         Enter solo = saltar ese control.  'q' + Enter = terminar y guardar.\n\
         Los que cambian el estado del teclado (Oct, Transp, Arp, Hold...)\n\
         podes volver a apretarlos despues de la lectura, antes del Enter.\n"
    );
    let mut map = String::from(
        "# KeyLab Essential mk3 MIDI input map\n\n\
         Read off the hardware with the `midi_map` example: each control\n\
         was pressed on its own and what arrived, on which port, is below.\n\n\
         | Control | Mensajes |\n|---|---|\n",
    );
    'controls: for control in CONTROLS {
        // What arrived while the last result was on screen is not this
        // control's.
        while receiver.try_recv().is_ok() {}
        print!("> {control}: ");
        std::io::stdout().flush()?;
        let first = loop {
            match receiver.recv() {
                Ok(Event::Midi(heard)) => break Some(heard),
                Ok(Event::Line(line)) if line.trim().eq_ignore_ascii_case("q") => {
                    println!("(fin)");
                    break 'controls;
                }
                Ok(Event::Line(_)) => break None,
                Err(_) => break 'controls,
            }
        };
        let Some(first) = first else {
            println!("(saltado)");
            let _ = writeln!(map, "| {control} | (saltado) |");
            continue;
        };
        let lines = summarize(&capture(&receiver, first));
        println!();
        for line in &lines {
            println!("      {line}");
        }
        let _ = writeln!(map, "| {control} | {} |", lines.join("<br>"));
        print!("  Enter para seguir ('q' para terminar)... ");
        std::io::stdout().flush()?;
        loop {
            match receiver.recv() {
                Ok(Event::Line(line)) => {
                    if line.trim().eq_ignore_ascii_case("q") {
                        break 'controls;
                    }
                    break;
                }
                Ok(Event::Midi(_)) => {}
                Err(_) => break 'controls,
            }
        }
    }
    std::fs::write("MIDI-MAP.md", &map)?;
    let path = std::env::current_dir()?.join("MIDI-MAP.md");
    println!("\nMapa guardado en {}", path.display());
    println!("Pulsa Enter para cerrar.");
    let _ = receiver.recv_timeout(Duration::from_secs(600));
    Ok(())
}
