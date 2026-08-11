use super::current_pan;
use rackforge_controller_api::HostControlTarget;
use rackforge_session_api::{MasterLevel, MasterPan};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HostControlEvent {
    pub(crate) target: HostControlTarget,
    pub(crate) value: u8,
}

impl HostControlEvent {
    pub(crate) fn header_text(self) -> String {
        match self.target {
            HostControlTarget::MasterLevel => {
                let level = MasterLevel::from_midi(self.value);
                let percent = (u32::from(level.get()) + 5) / 10;
                format!("MASTER VOL {:>7}", format!("{percent}%"))
            }
            HostControlTarget::MasterPan => {
                let pan = MasterPan::from_midi_with_center_snap(self.value);
                let value = pan.get();
                let label = if value == 0 {
                    "CENTER".to_owned()
                } else {
                    let side = if value < 0 { 'L' } else { 'R' };
                    let percent = (u32::from(value.unsigned_abs()) + 5) / 10;
                    format!("{side} {percent}%")
                };
                format!("MASTER PAN {:>7}", label)
            }
        }
    }
}

/// Moves the pan by how far the knob turned, not by where the knob sits.
///
/// The encoder reports an absolute position, and its position is not the one
/// RackForge holds: after a restart it sits near zero while the pan sits
/// wherever the session left it. Obeying that first reading slammed the mix to
/// whatever the knob happened to be pointing at.
///
/// Waiting for the knob to pass through the current value was the first
/// attempt and it was worse. With the pan at -283 the knob had to be hunted
/// down to a particular notch, and turning the wrong way did nothing at all
/// and said so only on a display nobody was looking at.
///
/// So the first reading is taken as a starting point and changes nothing.
/// Every reading after it moves the pan by the distance turned. The knob
/// starts from where the pan already is, which is what it looked like it
/// should do all along.
///
/// The fader is deliberately left alone. A fader is a position, and a player
/// who pushes it up expects the sound to follow it.
#[derive(Default)]
pub(crate) struct PanFollower {
    /// The previous reading, which is what a distance is measured from.
    previous: Option<u8>,
    /// Where the pan is, tracked here so that a sweep does not ask the core
    /// where it stands between every notch.
    ///
    /// This is the true position and not the reported one: inside the detent
    /// the pan is reported as centre while this keeps moving, which is what
    /// lets the knob pass through the middle instead of sticking to it.
    pan: Option<i16>,
    /// What was lost to division on previous notches, carried forward.
    ///
    /// A notch is worth 2000/127 of the range, which is not a whole number.
    /// Dropping the fraction each time costs about six per cent over a full
    /// sweep, and the pan could never be driven to either end.
    remainder: i32,
}

/// How far either side of centre still counts as centre.
///
/// Roughly six per cent of the travel, which is about four notches and close
/// to the dead zone the absolute reading used to have. Without it centre is
/// one exact value among two thousand and cannot be found by hand.
const PAN_DETENT: i16 = 60;

/// The pan a true position reports, with the middle widened into a detent.
///
/// The band around centre is not thrown away, it is folded in: what remains
/// on either side is stretched back over the whole range. Discarding it
/// instead made the pan leave the detent already at six or seven per cent,
/// because the first value outside the band is the width of the band.
///
/// Stretching keeps both ends reachable, which subtracting alone would not:
/// the travel lost to the detent has to come back somewhere.
fn through_detent(position: i16) -> i16 {
    let limit = i32::from(MasterPan::MAX);
    let detent = i32::from(PAN_DETENT);
    let magnitude = i32::from(position.abs());
    if magnitude <= detent {
        return 0;
    }
    let scaled = (magnitude - detent) * limit / (limit - detent);
    let scaled = scaled.min(limit) as i16;
    if position < 0 { -scaled } else { scaled }
}

impl PanFollower {
    /// The pan this reading moves to, or nothing when it moves nowhere.
    pub(crate) fn advance(&mut self, value: u8) -> Option<MasterPan> {
        let Some(previous) = self.previous.replace(value) else {
            // The first touch only says where the knob was. Acting on it is
            // exactly the jump this exists to prevent.
            self.pan.get_or_insert_with(|| current_pan().unwrap_or(0));
            return None;
        };
        let turned = i32::from(value) - i32::from(previous);
        if turned == 0 {
            return None;
        }
        let span = 2 * i32::from(MasterPan::MAX);
        // The carry is what makes a full sweep arrive at a full pan. Rust
        // keeps the sign of the dividend, so a remainder from turning one way
        // does not push the other way on the next notch.
        let scaled = turned * span + self.remainder;
        let moved = scaled / 127;
        self.remainder = scaled % 127;
        let pan = self.pan.unwrap_or(0);
        let limit = i32::from(MasterPan::MAX);
        let next = (i32::from(pan) + moved).clamp(-limit, limit) as i16;
        self.pan = Some(next);
        MasterPan::new(through_detent(next)).ok()
    }
}

/// The header shown while the pan is being turned.
pub(crate) fn pan_header_text(pan: MasterPan) -> String {
    let value = pan.get();
    let label = if value == 0 {
        "CENTER".to_owned()
    } else {
        let side = if value < 0 { 'L' } else { 'R' };
        format!("{side} {}%", (u32::from(value.unsigned_abs()) + 5) / 10)
    };
    format!("MASTER PAN {label:>7}")
}

pub(crate) fn coalesce_host_control_events(
    events: impl IntoIterator<Item = HostControlEvent>,
) -> Vec<HostControlEvent> {
    let mut latest_level = None;
    let mut latest_pan = None;
    let mut last_target = None;
    for event in events {
        match event.target {
            HostControlTarget::MasterLevel => latest_level = Some(event),
            HostControlTarget::MasterPan => latest_pan = Some(event),
        }
        last_target = Some(event.target);
    }

    let mut coalesced = Vec::with_capacity(2);
    match last_target {
        Some(HostControlTarget::MasterLevel) => {
            coalesced.extend(latest_pan);
            coalesced.extend(latest_level);
        }
        Some(HostControlTarget::MasterPan) => {
            coalesced.extend(latest_level);
            coalesced.extend(latest_pan);
        }
        None => {}
    }
    coalesced
}

#[cfg(test)]
mod pan_tests {
    use super::*;

    /// A follower that already knows where the pan is, so the tests do not
    /// need a running core to answer for it.
    fn following_from(pan: i16) -> PanFollower {
        PanFollower {
            previous: None,
            pan: Some(pan),
            remainder: 0,
        }
    }

    #[test]
    fn the_first_touch_moves_nothing() {
        // This is the whole bug: the encoder sat near zero while the pan sat
        // elsewhere, and obeying that first reading threw the mix across.
        let mut follower = following_from(-283);
        assert!(
            follower.advance(1).is_none(),
            "the first reading only anchors"
        );
        assert!(
            follower.advance(2).is_some(),
            "and every reading after it moves the pan"
        );
    }

    #[test]
    fn turning_moves_from_where_the_pan_already_was() {
        let mut follower = following_from(0);
        follower.advance(100);
        let pan = follower.advance(127).unwrap();
        assert!(pan.get() > 0, "turning up moved right: {}", pan.get());
    }

    #[test]
    fn a_knob_far_from_the_pan_still_works_immediately() {
        // The pickup attempt failed here: with the pan at -283 the knob had
        // to be hunted down to one notch, and turning the wrong way did
        // nothing. Now any movement moves the pan.
        let mut follower = following_from(-283);
        follower.advance(120);
        let pan = follower.advance(121).unwrap();
        assert!(pan.get() > -283, "it moved right from -283: {}", pan.get());
    }

    #[test]
    fn turning_back_returns_where_it_came_from() {
        let mut follower = following_from(0);
        follower.advance(60);
        let out = follower.advance(70).unwrap().get();
        let back = follower.advance(60).unwrap().get();
        assert!(out > 0, "{out}");
        assert_eq!(back, 0, "went to {out} and came back to {back}");
    }

    #[test]
    fn a_full_sweep_reaches_a_full_pan() {
        // A notch is worth 2000/127, which is not whole. Dropping the fraction
        // each time left the pan short of both ends — it stopped around 98%
        // and would not go further however long the knob was turned.
        for direction in [1_i32, -1] {
            let mut follower = following_from(0);
            let start = if direction > 0 { 0_u8 } else { 127 };
            follower.advance(start);
            let mut last = 0;
            for step in 1..=127_i32 {
                let reading = (i32::from(start) + direction * step).clamp(0, 127) as u8;
                if let Some(pan) = follower.advance(reading) {
                    last = pan.get();
                }
            }
            assert_eq!(
                last.abs(),
                MasterPan::MAX,
                "sweeping {} ended at {last}",
                if direction > 0 { "up" } else { "down" }
            );
        }
    }

    #[test]
    fn the_middle_is_a_place_the_knob_can_land_on() {
        // Centre is one value in two thousand. Without a detent it cannot be
        // found by hand, which is what the absolute reading's dead zone used
        // to provide.
        let mut follower = following_from(0);
        follower.advance(64);
        let nudged = follower.advance(65).unwrap();
        assert_eq!(nudged.get(), 0, "a notch off centre still reads as centre");
    }

    #[test]
    fn leaving_the_detent_starts_from_nothing_and_not_from_its_width() {
        // Discarding the band meant the first value outside it was the width
        // of it, so the pan jumped straight to six or seven per cent.
        assert_eq!(through_detent(PAN_DETENT), 0, "the edge is still centre");
        let just_out = through_detent(PAN_DETENT + 1);
        assert!(
            just_out > 0 && just_out < 10,
            "left the detent at {just_out}"
        );
        let mirrored = through_detent(-(PAN_DETENT + 1));
        assert_eq!(mirrored, -just_out, "both sides behave the same");
    }

    #[test]
    fn widening_the_middle_does_not_cost_the_ends() {
        assert_eq!(through_detent(MasterPan::MAX), MasterPan::MAX);
        assert_eq!(through_detent(-MasterPan::MAX), -MasterPan::MAX);
    }

    #[test]
    fn the_knob_passes_through_the_middle_rather_than_sticking_to_it() {
        // The detent reports centre while the true position keeps moving, so
        // turning far enough comes out the other side.
        let mut follower = following_from(0);
        follower.advance(64);
        let mut seen = Vec::new();
        for reading in 65..=76_u8 {
            if let Some(pan) = follower.advance(reading) {
                seen.push(pan.get());
            }
        }
        assert!(seen.contains(&0), "it held centre for a while: {seen:?}");
        assert!(
            seen.last().is_some_and(|last| *last > 0),
            "and then left it: {seen:?}"
        );
    }

    #[test]
    fn the_pan_cannot_be_pushed_past_its_ends() {
        let mut follower = following_from(MasterPan::MAX - 10);
        follower.advance(0);
        let pan = follower.advance(127).unwrap();
        assert_eq!(pan.get(), MasterPan::MAX);
    }

    #[test]
    fn a_still_knob_says_nothing() {
        let mut follower = following_from(0);
        follower.advance(64);
        assert!(follower.advance(64).is_none());
    }
}
