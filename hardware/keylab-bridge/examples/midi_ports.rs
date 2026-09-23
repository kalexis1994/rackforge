//! Lists every MIDI endpoint this machine exposes and what the KeyLab
//! matchers make of each one.
//!
//! The endpoint matchers were written and tested against ALSA names only,
//! so what Windows and CoreMIDI actually report has never been read back.
//! Guessing at it once already cost a round trip; this prints the strings
//! instead.
//!
//! `cargo run -p rackforge-controller-arturia-keylab-essential-mk3 --example midi_ports`
use midir::{MidiInput, MidiOutput};
use rackforge_controller_arturia_keylab_essential_mk3::controller;

fn report(kind: &str, name: &str) {
    let main = controller::is_main_midi_endpoint(name);
    let keylab = controller::is_keylab_endpoint(name);
    let display = controller::display_driver(name).is_some();
    let little = controller::little_driver(name).is_some();
    println!("  {kind:<4} {name:?}");
    println!(
        "       keylab={keylab}  main_endpoint={main}  display_driver={display}  LITTLE={little}"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = MidiInput::new("rackforge-midi-ports")?;
    let output = MidiOutput::new("rackforge-midi-ports")?;
    println!("entradas MIDI:");
    if input.ports().is_empty() {
        println!("  (ninguna)");
    }
    for port in input.ports() {
        report("in", &input.port_name(&port)?);
    }
    println!("\nsalidas MIDI:");
    if output.ports().is_empty() {
        println!("  (ninguna)");
    }
    for port in output.ports() {
        report("out", &output.port_name(&port)?);
    }
    println!(
        "\nLITTLE se envia por una SALIDA, asi que la linea que importa\n\
         es la de la salida del KeyLab con LITTLE=true."
    );
    Ok(())
}
