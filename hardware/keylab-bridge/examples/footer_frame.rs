//! Does the footer message blank the four LEDs under the screen?
//!
//! LED-MAP.md settled that `0x18`..`0x1B` are those buttons and that they
//! light when addressed on their own. They are still dark in normal use, and
//! the footer is the only message sent to them and to nothing else. It
//! carries a frame byte per button, `0x00` for both `Normal` and `Disabled`,
//! and a device that reads `0x00` as "no button here" would blank exactly
//! these four and nothing else.
//!
//! There is a second suspect with the same symptom. The driver re-sends the
//! footer on every screen update but re-sends a button LED only when its
//! bytes change, so if the footer blanks them they would light once and stay
//! dark from the next redraw onward.
//!
//! Six steps, each waiting for Return. Write down whether the four are lit
//! after each one; the pattern names the cause.
//!
//! `cargo run -p rackforge-controller-arturia-keylab-essential-mk3 --example footer_frame`
use midir::{MidiOutput, MidiOutputConnection};
use rackforge_controller_arturia_keylab_essential_mk3::{controller, protocol};
use rackforge_surface_runtime::FooterButton;
use rackforge_ui::VisualState;
use std::io::{BufRead, Write};

/// One numbered step: what it does to the device, and what to watch for.
type Step = (
    &'static str,
    Box<dyn Fn(&mut MidiOutputConnection) -> Result<(), Box<dyn std::error::Error>>>,
);

const LIT: [u8; 3] = [127, 127, 127];

fn footer_of(state: VisualState) -> [FooterButton; 4] {
    let labels = ["OK", "<", ">", "BACK"];
    std::array::from_fn(|index| FooterButton {
        label: labels[index].into(),
        state,
    })
}

fn send(port: &mut MidiOutputConnection, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    port.send(bytes)?;
    std::thread::sleep(std::time::Duration::from_millis(6));
    Ok(())
}

fn light_the_four(port: &mut MidiOutputConnection) -> Result<(), Box<dyn std::error::Error>> {
    for index in 0..4 {
        send(port, &protocol::button_led_message(index, LIT)?)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = MidiOutput::new("rackforge-footer-frame")?;
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
    println!("Usando {:?}\n", output.port_name(&port)?);
    let mut connection = output.connect(&port, "footer-frame")?;
    for message in protocol::acquire_messages()? {
        send(&mut connection, &message.bytes)?;
        std::thread::sleep(std::time::Duration::from_millis(u64::from(
            message.settle_after_ms,
        )));
    }

    let steps: [Step; 6] = [
        (
            "1. Los cuatro encendidos en blanco, sin footer.\n   Esperado: encendidos. Si no, el problema es anterior al footer",
            Box::new(light_the_four),
        ),
        (
            "2. Footer con frame 0x00 (Normal), sin retocar los LEDs.\n   Si se APAGAN aca, el footer es la causa",
            Box::new(|port| send(port, &protocol::footer(&footer_of(VisualState::Normal))?)),
        ),
        (
            "3. Reenvio los LEDs DESPUES de ese footer.\n   Si vuelven, es un problema de orden y no de frame",
            Box::new(light_the_four),
        ),
        (
            "4. Footer con frame 0x02 (Focused).\n   Si con este NO se apagan, el culpable es el valor 0x00",
            Box::new(|port| send(port, &protocol::footer(&footer_of(VisualState::Focused))?)),
        ),
        (
            "5. Otra vez footer 0x00, para confirmar que se repite",
            Box::new(|port| send(port, &protocol::footer(&footer_of(VisualState::Normal))?)),
        ),
        (
            "6. Lo que manda el driver de verdad: footer y LEDs juntos, en su orden",
            Box::new(|port| {
                for message in
                    protocol::render_messages(&rackforge_surface_runtime::Menu::default().render())?
                {
                    send(port, &message.bytes)?;
                }
                Ok(())
            }),
        ),
    ];

    println!("Return avanza. Anota si los CUATRO de abajo de la pantalla estan encendidos.\n");
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    for (description, action) in steps {
        println!("{description}");
        action(&mut connection)?;
        print!("   encendidos? [Return] ");
        std::io::stdout().flush()?;
        let _ = lines.next();
        println!();
    }
    for message in protocol::ambient_repaint_messages()? {
        send(&mut connection, &message.bytes)?;
    }
    println!("Listo. El teclado vuelve a su color ambiente.");
    Ok(())
}
