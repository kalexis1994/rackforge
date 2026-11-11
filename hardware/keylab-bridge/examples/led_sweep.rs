//! Walks the KeyLab's RGB LED IDs one at a time so the map can be read off
//! the hardware instead of guessed.
//!
//! The four context buttons under the screen are painted at `0x18`..`0x1B`,
//! and on a mk3 they stay dark while every other LED lights. Either those
//! IDs belong to something else or they are not addressable this way, and
//! there is no way to tell from here: the device has to answer.
//!
//! Lights one ID, prints it, waits for Return, moves on. Note which control
//! lights at which ID -- especially the four under the screen.
//!
//! `cargo run -p rackforge-controller-arturia-keylab-essential-mk3 --example led_sweep`
use midir::{MidiOutput, MidiOutputConnection};
use rackforge_controller_arturia_keylab_essential_mk3::{controller, protocol};
use std::io::{BufRead, Write};

/// Every ID the protocol will accept, `0x00` through `0x2B`.
const LAST_ID: u8 = 0x2B;
/// Bright white: whatever lights, lights unmistakably.
const ON: [u8; 3] = [127, 127, 127];
const OFF: [u8; 3] = [0, 0, 0];

fn send(port: &mut MidiOutputConnection, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    port.send(bytes)?;
    std::thread::sleep(std::time::Duration::from_millis(4));
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = MidiOutput::new("rackforge-led-sweep")?;
    let Some(port) = output.ports().into_iter().find(|port| {
        output
            .port_name(port)
            .is_ok_and(|name| controller::is_main_midi_endpoint(&name))
    }) else {
        println!("No encontre el puerto principal del KeyLab. Puertos vistos:");
        for port in output.ports() {
            println!("  {:?}", output.port_name(&port)?);
        }
        return Ok(());
    };
    let name = output.port_name(&port)?;
    println!("Usando {name:?}\n");
    let mut connection = output.connect(&port, "led-sweep")?;

    // The same handshake the driver performs, so the device is in the state
    // that accepts these messages rather than whatever it booted into.
    for message in protocol::acquire_messages()? {
        send(&mut connection, &message.bytes)?;
        std::thread::sleep(std::time::Duration::from_millis(u64::from(
            message.settle_after_ms,
        )));
    }
    // Everything dark, so exactly one lit LED is unambiguous.
    for id in 0..=LAST_ID {
        send(&mut connection, &protocol::rgb_led_message(id, OFF)?)?;
    }

    println!("Return avanza al siguiente ID, 'q' termina.");
    println!("Anota cual control se enciende en cada uno.\n");
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    for id in 0..=LAST_ID {
        send(&mut connection, &protocol::rgb_led_message(id, ON)?)?;
        let note = if (0x18..=0x1B).contains(&id) {
            "   <- uno de los cuatro que el codigo cree que son los botones"
        } else {
            ""
        };
        print!("  ID 0x{id:02X} ({id:>2}) encendido{note}  ");
        std::io::stdout().flush()?;
        let quit = matches!(lines.next(), Some(Ok(line)) if line.trim().eq_ignore_ascii_case("q"));
        send(&mut connection, &protocol::rgb_led_message(id, OFF)?)?;
        if quit {
            break;
        }
    }
    // Leave the keyboard as the driver would, not dark.
    for message in protocol::ambient_repaint_messages()? {
        send(&mut connection, &message.bytes)?;
    }
    println!("\nListo. El teclado vuelve a su color ambiente.");
    Ok(())
}
