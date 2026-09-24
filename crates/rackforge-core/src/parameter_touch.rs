//! The parameter a control last moved, for the screen that names it.
//!
//! A controller's link runs on the audio thread, where nothing may lock or
//! allocate, and the screen that should say "ROOM SIZE 1462 m3" lives on
//! another thread -- on the appliance, in another process. So every link, as
//! it moves a parameter, leaves the fact in [`PARAMETER_TOUCHES`]: which
//! instance (by a key of its id), which parameter, the value, and whether the
//! control is still on its way to the parameter (pickup). Readers poll it;
//! only the latest touch is kept, which is all a header shows.
//!
//! One writer -- the thread that applies links -- and any number of readers,
//! through a sequence lock: the sequence is odd while a touch is written, and
//! a reader that saw it change reads again.

use std::sync::atomic::{AtomicU64, Ordering, fence};

/// Where a control stands against the parameter it is linked to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TouchPickup {
    /// The control drives the parameter: `value` is what it set.
    Engaged,
    /// The control has not reached the parameter yet: `value` is where the
    /// parameter stands, and the control must be raised to it.
    MoveUp,
    /// As `MoveUp`, lowered.
    MoveDown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterTouch {
    /// [`instance_key`] of the instance or Rack Slot the link names.
    pub instance_key: u64,
    pub parameter_index: u32,
    pub value: f64,
    pub pickup: TouchPickup,
    /// The value the control stands at, in the parameter's units: `value`
    /// once engaged, and while it is on its way, where it has got to -- so
    /// a screen can show the fader closing in rather than a frozen number.
    pub control: f64,
}

/// The process's touches: every link records into it.
pub static PARAMETER_TOUCHES: ParameterTouchCell = ParameterTouchCell::new();

/// The latest touch, written by one thread and read by any.
pub struct ParameterTouchCell {
    sequence: AtomicU64,
    instance: AtomicU64,
    index_and_pickup: AtomicU64,
    value: AtomicU64,
    control: AtomicU64,
}

impl Default for ParameterTouchCell {
    fn default() -> Self {
        Self::new()
    }
}

impl ParameterTouchCell {
    pub const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            instance: AtomicU64::new(0),
            index_and_pickup: AtomicU64::new(0),
            value: AtomicU64::new(0),
            control: AtomicU64::new(0),
        }
    }

    /// Leaves a touch for the screen. Called by the thread that applies
    /// links; atomic stores and nothing else.
    pub fn record(&self, touch: ParameterTouch) {
        let pickup = match touch.pickup {
            TouchPickup::Engaged => 0_u64,
            TouchPickup::MoveUp => 1,
            TouchPickup::MoveDown => 2,
        };
        self.sequence.fetch_add(1, Ordering::AcqRel);
        self.instance.store(touch.instance_key, Ordering::Relaxed);
        self.index_and_pickup.store(
            u64::from(touch.parameter_index) | (pickup << 32),
            Ordering::Relaxed,
        );
        self.value.store(touch.value.to_bits(), Ordering::Relaxed);
        self.control
            .store(touch.control.to_bits(), Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
    }

    /// The latest touch, when there has been one since `seen`, which is
    /// moved on to it.
    pub fn latest(&self, seen: &mut u64) -> Option<ParameterTouch> {
        for _ in 0..8 {
            let before = self.sequence.load(Ordering::Acquire);
            if before == *seen || before == 0 {
                return None;
            }
            if before % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let instance_key = self.instance.load(Ordering::Relaxed);
            let packed = self.index_and_pickup.load(Ordering::Relaxed);
            let value = f64::from_bits(self.value.load(Ordering::Relaxed));
            let control = f64::from_bits(self.control.load(Ordering::Relaxed));
            fence(Ordering::Acquire);
            if self.sequence.load(Ordering::Relaxed) != before {
                continue;
            }
            *seen = before;
            let pickup = match packed >> 32 {
                1 => TouchPickup::MoveUp,
                2 => TouchPickup::MoveDown,
                _ => TouchPickup::Engaged,
            };
            return Some(ParameterTouch {
                instance_key,
                parameter_index: packed as u32,
                value,
                pickup,
                control,
            });
        }
        None
    }

    /// Where a reader starts to see only the touches made after now.
    pub fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::Acquire) & !1
    }
}

/// A stable key for an instance or Slot id, so the audio thread records no
/// string: FNV-1a over its bytes.
pub fn instance_key(instance_id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in instance_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Whether a touch names this instance or Slot. A link made in a Rack
/// editor names the Slot (`piano`), and a Rack voice is `rack.main/piano`;
/// both are tried, as [`crate::rack_graph::voice_matches_link_target`] does.
pub fn touch_names(touch: &ParameterTouch, instance_id: &str) -> bool {
    touch.instance_key == instance_key(instance_id)
        || instance_id
            .rsplit_once('/')
            .is_some_and(|(_, slot)| touch.instance_key == instance_key(slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_sees_the_latest_touch_once() {
        let cell = ParameterTouchCell::new();
        let mut seen = cell.current_sequence();
        assert_eq!(cell.latest(&mut seen), None);
        let key = instance_key("slot.piano");
        cell.record(ParameterTouch {
            instance_key: key,
            parameter_index: 23,
            value: 1462.5,
            pickup: TouchPickup::Engaged,
            control: 1462.5,
        });
        cell.record(ParameterTouch {
            instance_key: key,
            parameter_index: 23,
            value: 45.0,
            pickup: TouchPickup::MoveDown,
            control: 900.0,
        });
        let touch = cell.latest(&mut seen).unwrap();
        assert_eq!(touch.instance_key, key);
        assert_eq!(touch.parameter_index, 23);
        assert_eq!(touch.value, 45.0);
        assert_eq!(touch.pickup, TouchPickup::MoveDown);
        assert_eq!(touch.control, 900.0);
        assert_eq!(cell.latest(&mut seen), None);
        assert!(touch_names(&touch, "slot.piano"));
        assert!(touch_names(&touch, "rack.main/slot.piano"));
        assert!(!touch_names(&touch, "slot.organ"));
    }
}
