use rackforge_session_api::{HostControlTarget, MasterLevel, MasterPan};
use rackforge_surface_runtime::{FooterButton, Header, Input, Screen};
use rackforge_ui::VisualState;

pub const CONNECT: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x02, 0x0F, 0x40, 0x5A, 0x01, 0xF7,
];
pub const DISCONNECT: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x02, 0x0F, 0x40, 0x5A, 0x00, 0xF7,
];
pub const CLEAR_SCREEN: &[u8] = &[
    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x60, 0x61, 0x00, 0xF7,
];

const PREFIX: &[u8] = &[0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42];
pub const AMBIENT_LED_RGB: [u8; 3] = [10, 40, 64];
const RGB_LED_COUNT: u8 = 0x2C;
pub const PRESET_SETTLE_MS: u16 = 350;
pub const CONNECT_SETTLE_MS: u16 = 150;
pub const MESSAGE_SETTLE_MS: u16 = 20;
pub const LED_SETTLE_MS: u16 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundMessage {
    pub bytes: Vec<u8>,
    pub settle_after_ms: u16,
}

impl OutboundMessage {
    fn new(bytes: Vec<u8>, settle_after_ms: u16) -> Self {
        Self {
            bytes,
            settle_after_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputPhase {
    Press,
    Release,
    Turn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerEvent {
    Surface {
        input: Input,
        phase: InputPhase,
    },
    HostControl {
        target: HostControlTarget,
        value: u8,
    },
}

pub fn parse_input(message: &[u8]) -> Option<ControllerEvent> {
    let [status, controller, value] = message else {
        return None;
    };
    if *status & 0xF0 != 0xB0 {
        return None;
    }
    let surface = match (*controller, *value) {
        (44, 127) => Some((Input::Button1, InputPhase::Press)),
        (45, 127) => Some((Input::Button2, InputPhase::Press)),
        (46, 127) => Some((Input::Button3, InputPhase::Press)),
        (47, 127) => Some((Input::Button4, InputPhase::Press)),
        (44, 0) => Some((Input::Button1, InputPhase::Release)),
        (45, 0) => Some((Input::Button2, InputPhase::Release)),
        (46, 0) => Some((Input::Button3, InputPhase::Release)),
        (47, 0) => Some((Input::Button4, InputPhase::Release)),
        (116, 0..=63) => Some((Input::EncoderLeft, InputPhase::Turn)),
        (116, 65..=127) => Some((Input::EncoderRight, InputPhase::Turn)),
        (117, 127) => Some((Input::EncoderPress, InputPhase::Press)),
        (117, 0) => Some((Input::EncoderPress, InputPhase::Release)),
        (119, 127) => Some((Input::KeyboardParts, InputPhase::Press)),
        (119, 0) => Some((Input::KeyboardParts, InputPhase::Release)),
        _ => None,
    };
    if let Some((input, phase)) = surface {
        return Some(ControllerEvent::Surface { input, phase });
    }
    let target = match *controller {
        113 => HostControlTarget::MasterLevel,
        104 => HostControlTarget::MasterPan,
        _ => return None,
    };
    Some(ControllerEvent::HostControl {
        target,
        value: *value,
    })
}

pub fn host_control_header(target: HostControlTarget, value: u8) -> String {
    match target {
        HostControlTarget::MasterLevel => {
            let percent = (u32::from(MasterLevel::from_midi(value).get()) + 5) / 10;
            format!("MASTER VOL {:>7}", format!("{percent}%"))
        }
        HostControlTarget::MasterPan => {
            let pan = MasterPan::from_midi_with_center_snap(value).get();
            let label = if pan == 0 {
                "CENTER".to_owned()
            } else {
                let side = if pan < 0 { 'L' } else { 'R' };
                let percent = (u32::from(pan.unsigned_abs()) + 5) / 10;
                format!("{side} {percent}%")
            };
            format!("MASTER PAN {:>7}", label)
        }
    }
}

pub fn select_preset(index: u8) -> Result<Vec<u8>, String> {
    if index > 7 {
        return Err("KeyLab preset index must be between 0 and 7".into());
    }
    sysex(&[0x21, 0x11, 0x40, 0x02, 0x00, index])
}

pub fn screen_messages(screen: &Screen) -> Result<[Vec<u8>; 3], String> {
    Ok([
        two_lines(&screen.line_1, &screen.line_2)?,
        screen_header(&screen.header)?,
        footer(&screen.footer)?,
    ])
}

pub fn acquire_messages() -> Result<Vec<OutboundMessage>, String> {
    let mut messages = vec![
        OutboundMessage::new(select_preset(1)?, PRESET_SETTLE_MS),
        OutboundMessage::new(CONNECT.to_vec(), CONNECT_SETTLE_MS),
    ];
    messages.extend(
        all_rgb_led_messages(AMBIENT_LED_RGB)?
            .into_iter()
            .map(|bytes| OutboundMessage::new(bytes, LED_SETTLE_MS)),
    );
    Ok(messages)
}

pub fn initial_session_messages() -> Result<Vec<OutboundMessage>, String> {
    let mut messages = acquire_messages()?;
    messages.extend(render_messages(
        &rackforge_surface_runtime::Menu::default().render(),
    )?);
    Ok(messages)
}

pub fn render_messages(screen: &Screen) -> Result<Vec<OutboundMessage>, String> {
    let mut messages = screen_messages(screen)?
        .into_iter()
        .map(|bytes| OutboundMessage::new(bytes, MESSAGE_SETTLE_MS))
        .collect::<Vec<_>>();
    messages.extend(
        button_led_messages(&screen.footer)?
            .into_iter()
            .map(|bytes| OutboundMessage::new(bytes, LED_SETTLE_MS)),
    );
    Ok(messages)
}

pub fn transient_header_messages(text: &str) -> Result<Vec<OutboundMessage>, String> {
    Ok(vec![OutboundMessage::new(header(text)?, MESSAGE_SETTLE_MS)])
}

pub fn restore_messages() -> Result<Vec<OutboundMessage>, String> {
    let mut messages = all_rgb_led_messages([0, 0, 0])?
        .into_iter()
        .map(|bytes| OutboundMessage::new(bytes, 0))
        .collect::<Vec<_>>();
    messages.extend([
        OutboundMessage::new(CLEAR_SCREEN.to_vec(), 0),
        OutboundMessage::new(DISCONNECT.to_vec(), CONNECT_SETTLE_MS),
        OutboundMessage::new(select_preset(0)?, 0),
    ]);
    Ok(messages)
}

pub fn header(text: &str) -> Result<Vec<u8>, String> {
    let mut payload = vec![0x04, 0x01, 0x60, 0x01, 0x02];
    payload.extend_from_slice(ascii_text(text)?);
    payload.extend_from_slice(&[0x00, 0x00]);
    sysex(&payload)
}

pub fn screen_header(header: &Header) -> Result<Vec<u8>, String> {
    match header {
        Header::Visible(text) => self::header(text),
        Header::Hidden => sysex(&[0x04, 0x01, 0x60, 0x01, 0x00, 0x00]),
    }
}

pub fn two_lines(line_1: &str, line_2: &str) -> Result<Vec<u8>, String> {
    let mut payload = vec![0x04, 0x01, 0x60, 0x12, 0x01];
    payload.extend_from_slice(ascii_text(line_1)?);
    payload.extend_from_slice(&[0x00, 0x02]);
    payload.extend_from_slice(ascii_text(line_2)?);
    payload.extend_from_slice(&[0x00, 0x00]);
    sysex(&payload)
}

pub fn footer(buttons: &[FooterButton; 4]) -> Result<Vec<u8>, String> {
    let mut payload = vec![0x04, 0x01, 0x60, 0x03];
    for (index, button) in buttons.iter().enumerate() {
        let label = ascii_label(&button.label)?;
        let frame = match button.state {
            VisualState::Normal | VisualState::Disabled => 0x00,
            VisualState::Focused => 0x02,
            VisualState::Pressed => 0x03,
        };
        payload.push(0x10 + (index as u8 * 0x10));
        payload.extend_from_slice(&[frame, 0x00]);
        payload.push(0x11 + (index as u8 * 0x10));
        payload.extend_from_slice(label);
        payload.push(0x00);
    }
    sysex(&payload)
}

pub fn rgb_led_message(control_id: u8, rgb: [u8; 3]) -> Result<Vec<u8>, String> {
    if control_id >= RGB_LED_COUNT {
        return Err("KeyLab RGB control ID must be between 0x00 and 0x2B".into());
    }
    sysex(&[0x04, 0x01, 0x16, control_id, rgb[0], rgb[1], rgb[2]])
}

pub fn all_rgb_led_messages(rgb: [u8; 3]) -> Result<Vec<Vec<u8>>, String> {
    (0..RGB_LED_COUNT)
        .map(|control_id| rgb_led_message(control_id, rgb))
        .collect()
}

pub fn button_led_message(index: usize, rgb: [u8; 3]) -> Result<Vec<u8>, String> {
    if index >= 4 {
        return Err("KeyLab context-button LED index must be between 0 and 3".into());
    }
    rgb_led_message(0x18 + index as u8, rgb)
}

pub fn button_led_messages(buttons: &[FooterButton; 4]) -> Result<[Vec<u8>; 4], String> {
    let color = |state| match state {
        VisualState::Disabled => [0, 0, 0],
        VisualState::Normal => AMBIENT_LED_RGB,
        VisualState::Focused => [20, 80, 127],
        VisualState::Pressed => [127, 127, 127],
    };
    Ok([
        button_led_message(0, color(buttons[0].state))?,
        button_led_message(1, color(buttons[1].state))?,
        button_led_message(2, color(buttons[2].state))?,
        button_led_message(3, color(buttons[3].state))?,
    ])
}

pub fn clear_button_led_messages() -> [Vec<u8>; 4] {
    std::array::from_fn(|index| {
        button_led_message(index, [0, 0, 0]).expect("fixed KeyLab LED message is valid")
    })
}

fn sysex(payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.iter().any(|byte| *byte > 0x7F) {
        return Err("KeyLab SysEx payload contains non-7-bit data".into());
    }
    let mut message = Vec::with_capacity(PREFIX.len() + payload.len() + 1);
    message.extend_from_slice(PREFIX);
    message.extend_from_slice(payload);
    message.push(0xF7);
    Ok(message)
}

fn ascii_text(text: &str) -> Result<&[u8], String> {
    if text.is_empty() || text.len() > 18 || !text.is_ascii() || text.as_bytes().contains(&0) {
        return Err("KeyLab display text must be 1..=18 ASCII characters without NUL".into());
    }
    Ok(text.as_bytes())
}

fn ascii_label(text: &str) -> Result<&[u8], String> {
    if text.is_empty() || text.len() > 7 || !text.is_ascii() || text.as_bytes().contains(&0) {
        return Err("KeyLab footer labels must be 1..=7 ASCII characters without NUL".into());
    }
    Ok(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reserved_host_controls_and_surface_inputs() {
        assert_eq!(
            parse_input(&[0xB0, 113, 64]),
            Some(ControllerEvent::HostControl {
                target: HostControlTarget::MasterLevel,
                value: 64,
            })
        );
        assert_eq!(
            parse_input(&[0xB0, 44, 127]),
            Some(ControllerEvent::Surface {
                input: Input::Button1,
                phase: InputPhase::Press,
            })
        );
        assert_eq!(parse_input(&[0x90, 60, 100]), None);
        assert_eq!(
            parse_input(&[0xBF, 45, 127]),
            Some(ControllerEvent::Surface {
                input: Input::Button2,
                phase: InputPhase::Press,
            })
        );
    }

    #[test]
    fn preset_messages_match_the_keylab_protocol() {
        assert_eq!(
            select_preset(1).unwrap(),
            [
                0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x21, 0x11, 0x40, 0x02, 0x00, 0x01, 0xF7
            ]
        );
    }

    #[test]
    fn button_leds_follow_footer_availability() {
        let mut screen = rackforge_surface_runtime::Menu::default().render();
        screen.footer[1].state = VisualState::Disabled;
        assert_eq!(
            button_led_messages(&screen.footer).unwrap(),
            [
                vec![
                    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x16, 0x18, 10, 40, 64, 0xF7
                ],
                vec![
                    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x16, 0x19, 0, 0, 0, 0xF7
                ],
                vec![
                    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x16, 0x1A, 10, 40, 64, 0xF7
                ],
                vec![
                    0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42, 0x04, 0x01, 0x16, 0x1B, 10, 40, 64, 0xF7
                ],
            ]
        );
    }

    #[test]
    fn ambient_lighting_covers_buttons_and_all_sixteen_pads() {
        let messages = all_rgb_led_messages(AMBIENT_LED_RGB).unwrap();
        assert_eq!(messages.len(), 0x2C);
        assert_eq!(messages.first().unwrap()[9], 0x00);
        assert_eq!(messages.last().unwrap()[9], 0x2B);
        assert_eq!(&messages.last().unwrap()[10..13], &AMBIENT_LED_RGB);
    }

    #[test]
    fn portable_session_plan_owns_acquire_render_and_restore() {
        let acquire = acquire_messages().unwrap();
        assert_eq!(acquire[0].bytes, select_preset(1).unwrap());
        assert_eq!(acquire[0].settle_after_ms, PRESET_SETTLE_MS);
        assert_eq!(acquire[1].bytes, CONNECT);
        assert_eq!(acquire.len(), 2 + 0x2C);

        let render = render_messages(&rackforge_surface_runtime::Menu::default().render()).unwrap();
        assert_eq!(render.len(), 7);

        let restore = restore_messages().unwrap();
        assert_eq!(restore.len(), 0x2C + 3);
        assert_eq!(restore.last().unwrap().bytes, select_preset(0).unwrap());

        let initial = initial_session_messages().unwrap();
        assert_eq!(initial.len(), acquire.len() + render.len());
    }
}
