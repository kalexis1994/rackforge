//! Browser transport for the bundled Arturia `.rfcontroller`.
//!
//! The browser owns Web MIDI handles, but the controller state machine stays
//! here beside the browser host.  This reuses the same LITTLE menu and SysEx
//! renderer as the native hosts; JavaScript only forwards bytes and executes
//! the returned output plans.

use keylab_essential_mk3::protocol as keylab_protocol;
use rackforge_session_api::{MasterLevel, MasterPan, SessionState, SurfaceMode};
use rackforge_surface_runtime::{
    ActiveMode, Input, Menu, MenuCommand, PlayPlugin, PlaySound, Screen,
};
use serde::Serialize;
use std::time::{Duration, Instant};

const LONG_PRESS: Duration = Duration::from_millis(650);
const PART_LONG_PRESS: Duration = Duration::from_millis(1_500);
const HOME_CHORD_WINDOW: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Serialize)]
pub struct BrowserControllerOutput {
    pub bytes: Vec<u8>,
    pub settle_after_ms: u16,
}

#[derive(Debug)]
pub enum BrowserControllerAction {
    Menu(MenuCommand),
    MasterLevel(MasterLevel),
    MasterPan(MasterPan),
}

#[derive(Debug, Default)]
pub struct BrowserControllerOutcome {
    pub consumed: bool,
    pub actions: Vec<BrowserControllerAction>,
}

#[derive(Clone, Copy, Debug)]
struct HeldButton {
    pressed_at: Instant,
    long_emitted: bool,
}

#[derive(Debug, Default)]
struct GestureTracker {
    held: [Option<HeldButton>; 5],
    home_chord_emitted: bool,
}

impl GestureTracker {
    fn press(&mut self, input: Input, now: Instant) -> bool {
        let Some(index) = gesture_index(input) else {
            return false;
        };
        self.held[index].get_or_insert(HeldButton {
            pressed_at: now,
            long_emitted: false,
        });
        true
    }

    fn release(&mut self, input: Input, now: Instant) -> Option<Input> {
        let index = gesture_index(input)?;
        let held = self.held[index].take()?;
        let result = if held.long_emitted {
            None
        } else if now.saturating_duration_since(held.pressed_at) >= threshold(input) {
            input.long_press()
        } else {
            Some(input)
        };
        if self.held.iter().all(Option::is_none) {
            self.home_chord_emitted = false;
        }
        result
    }

    fn poll(&mut self, now: Instant) -> Vec<Input> {
        let mut result = Vec::new();
        if !self.home_chord_emitted
            && let (Some(ok), Some(back)) = (self.held[0], self.held[3])
        {
            let separation = if ok.pressed_at >= back.pressed_at {
                ok.pressed_at.duration_since(back.pressed_at)
            } else {
                back.pressed_at.duration_since(ok.pressed_at)
            };
            let started = ok.pressed_at.max(back.pressed_at);
            if separation <= HOME_CHORD_WINDOW
                && now.saturating_duration_since(started) >= LONG_PRESS
            {
                self.home_chord_emitted = true;
                self.held[0].as_mut().expect("OK is held").long_emitted = true;
                self.held[3].as_mut().expect("BACK is held").long_emitted = true;
                result.push(Input::HomeChord);
            }
        }
        for (index, held) in self.held.iter_mut().enumerate() {
            let Some(held) = held else { continue };
            let input = [
                Input::Button1,
                Input::Button2,
                Input::Button3,
                Input::Button4,
                Input::KeyboardParts,
            ][index];
            if !held.long_emitted
                && now.saturating_duration_since(held.pressed_at) >= threshold(input)
            {
                held.long_emitted = true;
                result.push(
                    input
                        .long_press()
                        .expect("tracked button has a long gesture"),
                );
            }
        }
        result
    }
}

fn threshold(input: Input) -> Duration {
    if input == Input::KeyboardParts {
        PART_LONG_PRESS
    } else {
        LONG_PRESS
    }
}

fn gesture_index(input: Input) -> Option<usize> {
    match input {
        Input::Button1 => Some(0),
        Input::Button2 => Some(1),
        Input::Button3 => Some(2),
        Input::Button4 => Some(3),
        Input::KeyboardParts => Some(4),
        _ => None,
    }
}

#[derive(Debug)]
pub struct BrowserKeyLabController {
    menu: Menu,
    connected: bool,
    last_screen: Option<Screen>,
    output: Vec<BrowserControllerOutput>,
    gestures: GestureTracker,
    pan_previous: Option<u8>,
    pan_position: i16,
    pan_remainder: i32,
}

impl Default for BrowserKeyLabController {
    fn default() -> Self {
        Self {
            menu: Menu::default(),
            connected: false,
            last_screen: None,
            output: Vec::new(),
            gestures: GestureTracker::default(),
            pan_previous: None,
            pan_position: 0,
            pan_remainder: 0,
        }
    }
}

impl BrowserKeyLabController {
    pub fn sync(&mut self, session: &SessionState) {
        self.menu.sync_active_mode(match session.active_mode {
            SurfaceMode::Idle => ActiveMode::Idle,
            SurfaceMode::Live => ActiveMode::Live,
            SurfaceMode::Play => ActiveMode::Play,
        });
        self.pan_position = session.master_pan.get();
        self.menu.set_play_plugins(
            session
                .instances
                .iter()
                .map(|instance| {
                    PlayPlugin::new(
                        instance.instance_id.as_str(),
                        &instance.plugin_id,
                        &instance.plugin_name,
                    )
                    .config_available(instance.config_available)
                })
                .collect(),
            session.active_instance_id.as_ref().map(|id| id.as_str()),
        );
        if let Some(active) = session.active_instance() {
            let sounds = active
                .sounds
                .iter()
                .map(|sound| {
                    PlaySound::new(
                        &sound.id,
                        &sound.name,
                        sound.bank.as_deref().unwrap_or("factory"),
                        sound.detail.as_deref().unwrap_or(" "),
                    )
                    .editable(sound.editable)
                })
                .collect();
            self.menu.sync_active_plugin(
                active.instance_id.as_str(),
                &active.plugin_id,
                &active.plugin_name,
                sounds,
                active.selected_sound_id.as_deref(),
            );
        }
        self.queue_current_screen(false);
    }

    pub fn connect(&mut self) {
        if self.connected {
            return;
        }
        self.connected = true;
        self.last_screen = None;
        self.queue_messages(keylab_protocol::acquire_messages());
        self.queue_current_screen(true);
    }

    pub fn disconnect(&mut self) {
        if !self.connected {
            return;
        }
        self.queue_messages(keylab_protocol::restore_messages());
        self.connected = false;
        self.last_screen = None;
        self.gestures = GestureTracker::default();
        self.pan_previous = None;
    }

    pub fn set_ambient_color(&mut self, rgb: [u8; 3]) {
        keylab_protocol::set_ambient_led_rgb([rgb[0] >> 1, rgb[1] >> 1, rgb[2] >> 1]);
        if self.connected {
            self.queue_messages(keylab_protocol::ambient_repaint_messages());
            self.last_screen = None;
            self.queue_current_screen(true);
        }
    }

    pub fn handle_midi(&mut self, message: &[u8]) -> BrowserControllerOutcome {
        let Some(event) = keylab_protocol::parse_input(message) else {
            return BrowserControllerOutcome::default();
        };
        let mut outcome = BrowserControllerOutcome {
            consumed: true,
            actions: Vec::new(),
        };
        match event {
            keylab_protocol::ControllerEvent::HostControl { target, value } => {
                use rackforge_session_api::HostControlTarget;
                match target {
                    HostControlTarget::MasterLevel => {
                        let level = MasterLevel::from_midi(value);
                        outcome
                            .actions
                            .push(BrowserControllerAction::MasterLevel(level));
                        self.queue_messages(keylab_protocol::transient_header_messages(
                            &keylab_protocol::host_control_header(target, value),
                        ));
                    }
                    HostControlTarget::MasterPan => {
                        if let Some(pan) = self.advance_pan(value) {
                            outcome
                                .actions
                                .push(BrowserControllerAction::MasterPan(pan));
                            self.queue_messages(keylab_protocol::transient_header_messages(
                                &format_pan_header(pan),
                            ));
                        }
                    }
                }
            }
            keylab_protocol::ControllerEvent::Surface { input, phase } => {
                use keylab_protocol::InputPhase;
                let now = Instant::now();
                let navigation = match phase {
                    InputPhase::Press if self.gestures.press(input, now) => {
                        if self.menu.set_button_pressed(input, true) {
                            self.queue_current_screen(true);
                        }
                        None
                    }
                    InputPhase::Release => {
                        let navigation = self.gestures.release(input, now);
                        if self.menu.set_button_pressed(input, false) {
                            self.queue_current_screen(true);
                        }
                        navigation
                    }
                    InputPhase::Turn => Some(input),
                    InputPhase::Press => Some(input),
                };
                if let Some(input) = navigation {
                    self.apply_input(input, &mut outcome.actions);
                }
            }
        }
        outcome
    }

    pub fn poll(&mut self) -> Vec<BrowserControllerAction> {
        let mut actions = Vec::new();
        for input in self.gestures.poll(Instant::now()) {
            self.apply_input(input, &mut actions);
        }
        actions
    }

    /// Finishes the navigation requested by a long BACK gesture.
    ///
    /// The menu deliberately waits for the host to confirm the activation so
    /// it can focus the canonical program returned by the runtime. Native
    /// hosts already perform this acknowledgement; without it the browser
    /// changed the session while LITTLE remained on its previous screen.
    pub fn complete_return_to_active_mode(
        &mut self,
        mode: ActiveMode,
        selected_sound_id: Option<&str>,
    ) {
        self.menu
            .complete_return_to_active_mode(mode, selected_sound_id);
        self.queue_current_screen(true);
    }

    pub fn has_output(&self) -> bool {
        !self.output.is_empty()
    }

    pub fn drain_output(&mut self) -> Vec<BrowserControllerOutput> {
        std::mem::take(&mut self.output)
    }

    fn apply_input(&mut self, input: Input, actions: &mut Vec<BrowserControllerAction>) {
        let screen = self.menu.apply_input_and_render(input);
        self.queue_screen(screen, true);
        if let Some(command) = self.menu.take_command() {
            actions.push(BrowserControllerAction::Menu(command));
        }
    }

    fn queue_current_screen(&mut self, force: bool) {
        self.queue_screen(self.menu.render(), force);
    }

    fn queue_screen(&mut self, screen: Screen, force: bool) {
        if !self.connected || (!force && self.last_screen.as_ref() == Some(&screen)) {
            return;
        }
        self.last_screen = Some(screen.clone());
        self.queue_messages(keylab_protocol::render_messages(&screen));
    }

    fn queue_messages(&mut self, messages: Result<Vec<keylab_protocol::OutboundMessage>, String>) {
        let Ok(messages) = messages else { return };
        self.output
            .extend(messages.into_iter().map(|message| BrowserControllerOutput {
                bytes: message.bytes,
                settle_after_ms: message.settle_after_ms,
            }));
    }

    fn advance_pan(&mut self, value: u8) -> Option<MasterPan> {
        let previous = self.pan_previous.replace(value)?;
        let turned = i32::from(value) - i32::from(previous);
        if turned == 0 {
            return None;
        }
        let span = 2 * i32::from(MasterPan::MAX);
        let scaled = turned * span + self.pan_remainder;
        let moved = scaled / 127;
        self.pan_remainder = scaled % 127;
        let limit = i32::from(MasterPan::MAX);
        self.pan_position = (i32::from(self.pan_position) + moved).clamp(-limit, limit) as i16;
        MasterPan::new(self.pan_position).ok()
    }
}

fn format_pan_header(pan: MasterPan) -> String {
    let value = pan.get();
    let label = if value == 0 {
        "CENTER".to_owned()
    } else {
        let side = if value < 0 { 'L' } else { 'R' };
        format!("{side} {}%", (u32::from(value.unsigned_abs()) + 5) / 10)
    };
    format!("MASTER PAN {label:>7}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_back_is_emitted_only_on_release() {
        let mut gestures = GestureTracker::default();
        let now = Instant::now();
        assert!(gestures.press(Input::Button4, now));
        assert!(gestures.poll(now + Duration::from_millis(200)).is_empty());
        assert_eq!(
            gestures.release(Input::Button4, now + Duration::from_millis(200)),
            Some(Input::Button4)
        );
    }

    #[test]
    fn the_emergency_chord_wins_over_individual_long_presses() {
        let mut gestures = GestureTracker::default();
        let now = Instant::now();
        gestures.press(Input::Button1, now);
        gestures.press(Input::Button4, now + Duration::from_millis(20));
        assert_eq!(
            gestures.poll(now + LONG_PRESS + Duration::from_millis(20)),
            vec![Input::HomeChord]
        );
    }

    #[test]
    fn long_back_is_emitted_once_while_the_button_is_still_held() {
        let mut gestures = GestureTracker::default();
        let now = Instant::now();
        assert!(gestures.press(Input::Button4, now));
        assert_eq!(gestures.poll(now + LONG_PRESS), vec![Input::Button4Long]);
        assert!(
            gestures
                .poll(now + LONG_PRESS + Duration::from_secs(1))
                .is_empty()
        );
        assert_eq!(
            gestures.release(Input::Button4, now + LONG_PRESS + Duration::from_secs(2)),
            None
        );
    }
}
