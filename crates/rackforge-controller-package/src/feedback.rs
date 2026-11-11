//! Schema 2's feedback layer: what RackForge says to a controller.
//!
//! Two exchanges need no driver. The Universal SysEx Identity Request asks a
//! device which model it is, so a package can claim it by its answer instead
//! of by a port name another model may share. And a package may list messages
//! sent when the controller connects -- the SysEx that puts a keyboard in the
//! mode its package describes. Sending anything is layer 4: the package asks
//! for MIDI output, and SysEx when it sends SysEx, and the player allows it.

use crate::SysexIdentity;
use serde::{Deserialize, Serialize};

/// Universal Non-Realtime, all devices, General Information, Identity Request.
pub const IDENTITY_REQUEST: [u8; 6] = [0xf0, 0x7e, 0x7f, 0x06, 0x01, 0xf7];

/// How long a host waits for Identity Replies before it binds by name alone.
pub const IDENTITY_REPLY_WINDOW_MS: u64 = 400;

const MAX_ON_CONNECT_MESSAGES: usize = 32;
const MAX_MESSAGE_BYTES: usize = 1024;

/// What a device answered to the Identity Request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IdentityReply {
    /// One byte, or three beginning with 0x00.
    pub manufacturer: Vec<u8>,
    pub family: u16,
    pub model: u16,
    /// The four software revision bytes, as sent.
    pub version: [u8; 4],
}

impl IdentityReply {
    /// Reads `F0 7E <device> 06 02 <manufacturer> <family> <model> <version> F7`.
    /// Anything else, including a reply cut short, is not an Identity Reply.
    pub fn parse(message: &[u8]) -> Option<Self> {
        let body = message.strip_prefix(&[0xf0, 0x7e])?.strip_suffix(&[0xf7])?;
        let [device, 0x06, 0x02, rest @ ..] = body else {
            return None;
        };
        if *device > 0x7f || rest.iter().any(|byte| *byte > 0x7f) {
            return None;
        }
        let (manufacturer, rest) = match rest {
            [0x00, high, low, rest @ ..] => (vec![0x00, *high, *low], rest),
            [id, rest @ ..] => (vec![*id], rest),
            [] => return None,
        };
        let [family_lsb, family_msb, model_lsb, model_msb, a, b, c, d] = rest else {
            return None;
        };
        Some(Self {
            manufacturer,
            family: u16::from(*family_lsb) | (u16::from(*family_msb) << 7),
            model: u16::from(*model_lsb) | (u16::from(*model_msb) << 7),
            version: [*a, *b, *c, *d],
        })
    }
}

impl SysexIdentity {
    pub fn matches(&self, reply: &IdentityReply) -> bool {
        self.manufacturer == reply.manufacturer
            && self.family == reply.family
            && self.model == reply.model
    }
}

/// One message a package sends when its controller connects, written as hex
/// bytes: `"F0 00 20 6B 7F 42 02 00 40 50 01 F7"` or `"B0 7F 00"`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnConnectMessage {
    pub message: String,
    /// Where it goes: the controller's own port, or the port the package
    /// declares as `setup_output` -- a DAW port, on controllers that change
    /// mode only through it.
    #[serde(default, skip_serializing_if = "OnConnectPort::is_input")]
    pub to: OnConnectPort,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnConnectPort {
    /// The output of the port the controller was found on.
    #[default]
    Input,
    /// The output the package's `setup_output` endpoint names.
    SetupOutput,
}

impl OnConnectPort {
    fn is_input(&self) -> bool {
        *self == Self::Input
    }
}

impl OnConnectMessage {
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        let bytes = parse_hex_bytes(&self.message)?;
        validate_output_message(&bytes)?;
        Ok(bytes)
    }
}

/// Checks a package's messages; the permissions they need follow from them.
pub(crate) fn validate_on_connect(messages: &[OnConnectMessage]) -> Result<(), String> {
    if messages.len() > MAX_ON_CONNECT_MESSAGES {
        return Err(format!(
            "a package sends at most {MAX_ON_CONNECT_MESSAGES} messages on connect"
        ));
    }
    for (index, message) in messages.iter().enumerate() {
        message
            .bytes()
            .map_err(|error| format!("on_connect message {}: {error}", index + 1))?;
    }
    Ok(())
}

pub(crate) fn sends_sysex(messages: &[OnConnectMessage]) -> bool {
    messages.iter().any(|message| {
        message
            .bytes()
            .is_ok_and(|bytes| bytes.first() == Some(&0xf0))
    })
}

fn parse_hex_bytes(text: &str) -> Result<Vec<u8>, String> {
    let bytes = text
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|token| !token.is_empty())
        .map(|token| {
            let digits = token
                .strip_prefix("0x")
                .or_else(|| token.strip_prefix("0X"))
                .unwrap_or(token);
            if digits.is_empty() || digits.len() > 2 {
                return Err(format!("{token:?} is not one hex byte"));
            }
            u8::from_str_radix(digits, 16).map_err(|_| format!("{token:?} is not one hex byte"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if bytes.is_empty() {
        return Err("the message is empty".into());
    }
    Ok(bytes)
}

/// One complete message a package may send: a channel message, or one SysEx
/// message. System common and realtime messages -- clock, start, stop,
/// reset -- are the host's to send, never a package's.
fn validate_output_message(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(format!("a message is at most {MAX_MESSAGE_BYTES} bytes"));
    }
    let status = bytes[0];
    if status == 0xf0 {
        if bytes.len() < 3 || bytes[bytes.len() - 1] != 0xf7 {
            return Err("a SysEx message starts with F0 and ends with F7".into());
        }
        if bytes[1..bytes.len() - 1].iter().any(|byte| *byte > 0x7f) {
            return Err("SysEx data bytes are 7-bit".into());
        }
        return Ok(());
    }
    let length = match status & 0xf0 {
        0x80 | 0x90 | 0xa0 | 0xb0 | 0xe0 => 3,
        0xc0 | 0xd0 => 2,
        0xf0 => return Err("system common and realtime messages are the host's to send".into()),
        _ => return Err("a message starts with a status byte".into()),
    };
    if bytes.len() != length {
        return Err(format!(
            "a message with status {status:02X} is {length} bytes, not {}",
            bytes.len()
        ));
    }
    if bytes[1..].iter().any(|byte| *byte > 0x7f) {
        return Err("data bytes are 7-bit".into());
    }
    Ok(())
}

/// Whether RackForge sends a package's messages to its controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputState {
    /// The package sends nothing.
    None,
    /// The package asks to send, and the player has not allowed it for this
    /// version.
    Asked,
    /// Allowed, by the player or because RackForge ships the package.
    Allowed,
}

/// A package's feedback layer as a controller catalog lists it: whether it
/// may send, and what.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputSummary {
    pub state: OutputState,
    /// The on_connect messages, as the package writes them.
    pub messages: Vec<String>,
    /// Whether the package asks to send SysEx.
    pub sysex: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> OnConnectMessage {
        OnConnectMessage {
            message: text.into(),
            to: OnConnectPort::Input,
        }
    }

    #[test]
    fn reads_one_and_three_byte_identity_replies() {
        let roland = IdentityReply::parse(&[
            0xf0, 0x7e, 0x10, 0x06, 0x02, 0x41, 0x1a, 0x02, 0x03, 0x00, 0x01, 0x02, 0x03, 0x04,
            0xf7,
        ])
        .unwrap();
        assert_eq!(roland.manufacturer, vec![0x41]);
        assert_eq!(roland.family, 0x011a);
        assert_eq!(roland.model, 0x0003);
        assert_eq!(roland.version, [1, 2, 3, 4]);

        let arturia = IdentityReply::parse(&[
            0xf0, 0x7e, 0x7f, 0x06, 0x02, 0x00, 0x20, 0x6b, 0x02, 0x00, 0x05, 0x72, 0x00, 0x00,
            0x01, 0x02, 0xf7,
        ])
        .unwrap();
        assert_eq!(arturia.manufacturer, vec![0x00, 0x20, 0x6b]);
        assert_eq!(arturia.family, 0x0002);
        assert_eq!(arturia.model, 0x3905);
        let identity = SysexIdentity {
            manufacturer: vec![0x00, 0x20, 0x6b],
            family: 0x0002,
            model: 0x3905,
        };
        assert!(identity.matches(&arturia));
        assert!(!identity.matches(&roland));
    }

    #[test]
    fn anything_but_a_complete_identity_reply_is_ignored() {
        assert_eq!(IdentityReply::parse(&IDENTITY_REQUEST), None);
        assert_eq!(IdentityReply::parse(&[0xb0, 0x07, 0x40]), None);
        // Cut short by one version byte.
        assert_eq!(
            IdentityReply::parse(&[
                0xf0, 0x7e, 0x10, 0x06, 0x02, 0x41, 0x1a, 0x02, 0x03, 0x00, 0x01, 0x02, 0x03, 0xf7
            ]),
            None
        );
        // A data byte with the high bit set.
        assert_eq!(
            IdentityReply::parse(&[
                0xf0, 0x7e, 0x10, 0x06, 0x02, 0x41, 0x9a, 0x02, 0x03, 0x00, 0x01, 0x02, 0x03, 0x04,
                0xf7
            ]),
            None
        );
    }

    #[test]
    fn on_connect_messages_are_complete_channel_or_sysex_messages() {
        assert_eq!(
            message("F0 00 20 6B 7F 42 02 00 40 50 01 F7")
                .bytes()
                .unwrap(),
            vec![
                0xf0, 0x00, 0x20, 0x6b, 0x7f, 0x42, 0x02, 0x00, 0x40, 0x50, 0x01, 0xf7
            ]
        );
        assert_eq!(
            message("0xB0, 0x7F, 0x00").bytes().unwrap(),
            vec![0xb0, 0x7f, 0x00]
        );
        assert_eq!(message("c0 05").bytes().unwrap(), vec![0xc0, 0x05]);
        for bad in [
            "",
            "F0 00 20",
            "F0 80 F7",
            "B0 7F",
            "B0 7F 00 00",
            "FF",
            "F8",
            "40 00",
            "B0 7G 00",
            "B0 100 00",
        ] {
            assert!(message(bad).bytes().is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_package_sends_a_bounded_number_of_messages() {
        let many = vec![message("B0 7F 00"); MAX_ON_CONNECT_MESSAGES + 1];
        assert!(validate_on_connect(&many).is_err());
        assert!(validate_on_connect(&many[..MAX_ON_CONNECT_MESSAGES]).is_ok());
        assert!(!sends_sysex(&many));
        assert!(sends_sysex(&[message("B0 7F 00"), message("F0 7D 01 F7")]));
    }
}
