//! The Concert Grand laboratory: the instrument alone, natively, with the
//! audio device and the MIDI keyboard plugged straight into it, and every
//! constant of the model in a text file that is re-read the moment it is
//! saved. New notes take the new values; nothing is rebuilt.
//!
//!     cargo run --release -p rackforge-concert-grand --example lab -- [--list]
//!         [--out <device substring>] [--midi <port substring>]
//!         [--tuning <file>] [--render <score.txt> <out.wav>]
//!         [--foreground] [--no-edit] [--stop]
//!
//! By default the lab detaches: it relaunches itself in the background (log
//! in `%LOCALAPPDATA%\RackForge\lab.log`, pid in `lab.pid`), opens the tuning
//! file in the default editor, and gives the terminal back. `--stop` ends the
//! running lab; starting a new one replaces it. `--foreground` keeps it in
//! the terminal, `--no-edit` leaves the editor closed.
//!
//! The tuning file is created with every knob at its shipped value and its
//! documentation the first time; lines read `NAME = value`, and a line
//! `fader.<index> = 0..1` sets one of the instrument's own parameters (the
//! indices are those of `metadata/parameters.json`). A score for `--render`
//! is one note per line, `onset_ms duration_ms note velocity`, rendered in
//! stereo at 48 kHz with three seconds of tail.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rackforge_concert_grand::{ConcertGrand, apply_tuning, dump_tuning};
use rackforge_plugin_sdk::{MidiEvent, Processor};

const BLOCK: usize = 512;

struct Options {
    list: bool,
    foreground: bool,
    edit: bool,
    stop: bool,
    out: Option<String>,
    midi: Option<String>,
    tuning: PathBuf,
    render: Option<(PathBuf, PathBuf)>,
}

fn options() -> Options {
    let mut args = std::env::args().skip(1);
    let mut options = Options {
        list: false,
        foreground: false,
        edit: true,
        stop: false,
        out: None,
        midi: None,
        tuning: default_tuning_path(),
        render: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => options.list = true,
            "--foreground" => options.foreground = true,
            "--no-edit" => options.edit = false,
            "--stop" => options.stop = true,
            "--out" => options.out = args.next(),
            "--midi" => options.midi = args.next(),
            "--tuning" => options.tuning = PathBuf::from(args.next().expect("--tuning <file>")),
            "--render" => {
                let score = PathBuf::from(args.next().expect("--render <score> <wav>"));
                let wav = PathBuf::from(args.next().expect("--render <score> <wav>"));
                options.render = Some((score, wav));
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    options
}

fn app_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("RackForge")
}

fn default_tuning_path() -> PathBuf {
    app_dir().join("concert-grand.tuning")
}

/// Ends a lab left running by an earlier launch, by the pid it wrote.
fn stop_running_lab() -> bool {
    let pid_file = app_dir().join("lab.pid");
    let Ok(text) = std::fs::read_to_string(&pid_file) else {
        return false;
    };
    let Ok(pid) = text.trim().parse::<u32>() else {
        return false;
    };
    if pid == std::process::id() {
        return false;
    }
    let killed = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let _ = std::fs::remove_file(&pid_file);
    killed
}

/// Relaunches this executable detached from the terminal, with the same
/// arguments plus `--foreground`, its output in the log file.
fn detach() {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().expect("own path");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let log = app_dir().join("lab.log");
    let _ = std::fs::create_dir_all(app_dir());
    let file = std::fs::File::create(&log).expect("cannot create the log");
    let err = file.try_clone().expect("log");
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let child = std::process::Command::new(exe)
        .args(&args)
        .arg("--foreground")
        .stdin(std::process::Stdio::null())
        .stdout(file)
        .stderr(err)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .expect("cannot relaunch detached");
    println!(
        "lab: running detached as pid {} (log {}); stop it with --stop",
        child.id(),
        log.display()
    );
}

fn open_in_editor(path: &Path) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .spawn();
}

/// Reads the tuning file into the knobs; returns the fader lines to apply.
fn load_tuning(path: &Path, announce: bool) -> Vec<(usize, f32)> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (set, faders, complaints) = apply_tuning(&text);
            if announce {
                println!(
                    "tuning: {set} knobs, {} faders from {}",
                    faders.len(),
                    path.display()
                );
            }
            for complaint in complaints {
                println!("tuning: {complaint}");
            }
            faders
        }
        Err(error) => {
            println!("tuning: cannot read {}: {error}", path.display());
            Vec::new()
        }
    }
}

fn ensure_tuning_file(path: &Path) {
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(path, dump_tuning()) {
        Ok(()) => println!(
            "tuning: wrote every knob at its shipped value to {}",
            path.display()
        ),
        Err(error) => println!("tuning: cannot write {}: {error}", path.display()),
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn apply_faders(piano: &mut ConcertGrand, faders: &[(usize, f32)]) {
    for (index, value) in faders {
        if !piano.set_parameter(*index as u32, *value as f64) {
            println!("fader.{index}: the instrument refused {value}");
        }
    }
}

fn write_wav_stereo(path: &Path, rate: u32, samples: &[f32]) -> std::io::Result<()> {
    use std::io::Write;
    let frames = samples.len() / 2;
    let data_bytes = (frames * 2 * 3) as u32;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 6).to_le_bytes());
    out.extend_from_slice(&6u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        let v = (sample.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
        out.extend_from_slice(&v.to_le_bytes()[..3]);
    }
    std::fs::File::create(path)?.write_all(&out)
}

fn render(score: &Path, wav: &Path, tuning: &Path) {
    let rate = 48_000u32;
    let text = std::fs::read_to_string(score).expect("cannot read the score");
    let mut events: Vec<(u64, [u8; 3])> = Vec::new();
    let mut last_ms = 0u64;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 || f[0].starts_with('#') {
            continue;
        }
        let onset: u64 = f[0].parse().expect("onset_ms");
        let duration: u64 = f[1].parse().expect("duration_ms");
        let note: u8 = f[2].parse().expect("note");
        let velocity: u8 = f[3].parse().expect("velocity");
        events.push((onset, [0x90, note, velocity]));
        events.push((onset + duration, [0x80, note, 64]));
        last_ms = last_ms.max(onset + duration);
    }
    events.sort_by_key(|(at, _)| *at);
    let mut piano = Box::new(ConcertGrand::default());
    assert!(piano.prepare(rate as f64, BLOCK as u32, 0, 2));
    let faders = load_tuning(tuning, true);
    apply_faders(&mut piano, &faders);
    let total = ((last_ms + 3_000) * rate as u64 / 1000) as usize;
    let mut output = Vec::with_capacity(total * 2);
    let mut next = 0;
    let mut frame = 0usize;
    let mut block = vec![0.0f32; BLOCK * 2];
    while frame < total {
        let frames = BLOCK.min(total - frame);
        let mut midi = Vec::new();
        while next < events.len() {
            let at = (events[next].0 * rate as u64 / 1000) as usize;
            if at >= frame + frames {
                break;
            }
            midi.push(MidiEvent {
                frame: at.saturating_sub(frame) as u32,
                data: events[next].1,
                length: 3,
            });
            next += 1;
        }
        block[..frames * 2].fill(0.0);
        piano.process(
            &[],
            &mut block[..frames * 2],
            &midi,
            &[],
            frames as u32,
            0,
            2,
        );
        output.extend_from_slice(&block[..frames * 2]);
        frame += frames;
    }
    write_wav_stereo(wav, rate, &output).expect("cannot write the wav");
    println!(
        "rendered {} s to {}",
        total as f32 / rate as f32,
        wav.display()
    );
}

fn main() {
    let options = options();
    if options.stop {
        println!(
            "lab: {}",
            if stop_running_lab() {
                "stopped"
            } else {
                "nothing was running"
            }
        );
        return;
    }
    ensure_tuning_file(&options.tuning);
    if let Some((score, wav)) = &options.render {
        render(score, wav, &options.tuning);
        return;
    }
    if !options.list && !options.foreground {
        stop_running_lab();
        detach();
        if options.edit {
            open_in_editor(&options.tuning);
        }
        return;
    }
    if !options.list {
        stop_running_lab();
        let _ = std::fs::create_dir_all(app_dir());
        let _ = std::fs::write(app_dir().join("lab.pid"), std::process::id().to_string());
    }

    let host = cpal::default_host();
    let midi_in = midir::MidiInput::new("rackforge-lab").expect("midi");
    if options.list {
        println!("audio outputs:");
        for device in host.output_devices().expect("devices") {
            println!("  {}", device.name().unwrap_or_default());
        }
        println!("midi inputs:");
        for port in midi_in.ports() {
            println!("  {}", midi_in.port_name(&port).unwrap_or_default());
        }
        return;
    }

    let device = match &options.out {
        Some(wanted) => host
            .output_devices()
            .expect("devices")
            .find(|d| {
                d.name()
                    .map(|n| n.to_lowercase().contains(&wanted.to_lowercase()))
                    .unwrap_or(false)
            })
            .expect("no output device matches --out"),
        None => host
            .default_output_device()
            .expect("no default output device"),
    };
    let config = device.default_output_config().expect("output config");
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    println!(
        "audio: {} at {rate} Hz, {channels} ch",
        device.name().unwrap_or_default()
    );

    let (midi_tx, midi_rx) = mpsc::channel::<[u8; 3]>();
    let (fader_tx, fader_rx) = mpsc::channel::<(usize, f32)>();
    let ports = midi_in.ports();
    let port = match &options.midi {
        Some(wanted) => ports
            .iter()
            .find(|p| {
                midi_in
                    .port_name(p)
                    .map(|n| n.to_lowercase().contains(&wanted.to_lowercase()))
                    .unwrap_or(false)
            })
            .cloned(),
        None => ports.first().cloned(),
    };
    let _midi_connection = match port {
        Some(port) => {
            println!("midi: {}", midi_in.port_name(&port).unwrap_or_default());
            let tx = midi_tx.clone();
            Some(
                midi_in
                    .connect(
                        &port,
                        "rackforge-lab",
                        move |_, message, _| {
                            if message.len() >= 2 {
                                let mut data = [0u8; 3];
                                data[..message.len().min(3)]
                                    .copy_from_slice(&message[..message.len().min(3)]);
                                let _ = tx.send(data);
                            }
                        },
                        (),
                    )
                    .expect("cannot open the midi port"),
            )
        }
        None => {
            println!("midi: no input port (use --list, --midi)");
            None
        }
    };

    let mut piano = Box::new(ConcertGrand::default());
    assert!(piano.prepare(rate as f64, BLOCK as u32, 0, 2));
    for fader in load_tuning(&options.tuning, true) {
        let _ = fader_tx.send(fader);
    }

    let mut scratch = vec![0.0f32; BLOCK * 2];
    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let mut events: Vec<MidiEvent> = Vec::new();
                while let Ok(bytes) = midi_rx.try_recv() {
                    events.push(MidiEvent {
                        frame: 0,
                        data: bytes,
                        length: 3,
                    });
                }
                while let Ok((index, value)) = fader_rx.try_recv() {
                    if !piano.set_parameter(index as u32, value as f64) {
                        eprintln!("fader.{index}: refused {value}");
                    }
                }
                let total = data.len() / channels;
                let mut frame = 0;
                while frame < total {
                    let frames = BLOCK.min(total - frame);
                    scratch[..frames * 2].fill(0.0);
                    let midi = if frame == 0 { events.as_slice() } else { &[] };
                    piano.process(
                        &[],
                        &mut scratch[..frames * 2],
                        midi,
                        &[],
                        frames as u32,
                        0,
                        2,
                    );
                    for i in 0..frames {
                        let base = (frame + i) * channels;
                        data[base] = scratch[i * 2];
                        if channels > 1 {
                            data[base + 1] = scratch[i * 2 + 1];
                        }
                        for extra in 2..channels {
                            data[base + extra] = 0.0;
                        }
                    }
                    frame += frames;
                }
            },
            |error| eprintln!("audio: {error}"),
            None,
        )
        .expect("cannot open the audio stream");
    stream.play().expect("cannot start the audio stream");
    println!(
        "playing. Edit and save {} to hear a change on the next note. Ctrl+C to quit.",
        options.tuning.display()
    );

    let mut stamp = modified(&options.tuning);
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let now = modified(&options.tuning);
        if now != stamp {
            stamp = now;
            std::thread::sleep(Duration::from_millis(50));
            for fader in load_tuning(&options.tuning, true) {
                let _ = fader_tx.send(fader);
            }
        }
    }
}
