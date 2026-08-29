//! The sequencer engine: patterns into sample-accurate MIDI.
//!
//! The second piece of the live sequencer, on top of [`crate::transport`].
//! Like the transport it lives outside every platform gate — the browser
//! worklet runs the same engine the Pi does — and like everything on the
//! audio path it is split in two:
//!
//! * **Documents** ([`PatternDocument`]) are what surfaces edit and the
//!   performance library stores: serde types, positions in integer ticks so
//!   a saved pattern has no float in it to disagree about.
//! * **Runtime** ([`CompiledPattern`], [`SequencerLane`]) is what the audio
//!   callback consumes: validated, sorted, allocation-free per block.
//!   Compilation happens off the audio thread; the callback only walks.
//!
//! # Live invariants
//!
//! **Launches are quantised by the caller's beat, executed by the engine's
//! sample.** [`SequencerLane::queue`] takes an absolute beat (the caller gets
//! it from [`crate::transport::Transport::next_bar`] or its own phrase
//! arithmetic); the engine begins the pattern at exactly that beat's frame,
//! inside whichever block crosses it. Switching patterns never stops audio.
//!
//! **Note-offs are debts, not events.** Every emitted note-on records its off
//! as a debt with an absolute beat; debts are paid on time across pattern
//! switches, loop wraps and stops, and [`SequencerLane::panic_into`] pays all
//! of them immediately. Under output pressure note-*ons* are dropped and
//! counted — headroom for offs is reserved — so a saturated block degrades to
//! thinner music, never to stuck notes.
//!
//! **Determinism.** The same documents, launches and blocks produce the same
//! MIDI, bit for bit. There is no randomness and no wall clock here; time
//! only enters as the beat positions the transport already fixed.
//!
//! **A lane speaks on its own MIDI channel** — lane N emits on wire channel
//! N, which musicians read as channel N+1. That single rule is what lets a
//! Rack cable lanes to Slots with the same channel filters it already uses
//! for keyboards, instead of growing a second routing system. The per-note
//! `channel` field in the document is reserved; emission is the lane's.

use crate::transport::{TimeSignature, Transport};
use rackforge_control_api::{
    CapturedNoteV1, SequencerCommand, SequencerLaneStatus, SequencerQuantize, SequencerScale,
    SequencerStatusV1,
};
use rackforge_performance_api::{
    FollowAction, MAX_NOTE_LOCKS, MAX_PART_PATTERN_BINDINGS, MAX_PATTERN_NOTES,
    MAX_PATTERN_TICKS, PATTERN_SWING_MAX, PATTERN_SWING_STRAIGHT, PATTERN_TICKS_PER_BEAT,
    PatternDefinition, SongPart, TrigCondition,
};
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};
use std::sync::Arc;

/// Tick resolution of pattern documents, re-exported from the library schema:
/// the durable shape and the playable shape count time identically.
pub const TICKS_PER_BEAT: u32 = PATTERN_TICKS_PER_BEAT;

/// Most events one lane emits per block. Matches the host's own per-block
/// event ceiling; the tail is reserved for note-offs.
pub const MAX_EVENTS_PER_BLOCK: usize = 256;

/// Most simultaneously sounding notes one lane tracks. A note-on past this
/// is dropped and counted rather than sounding with no way to end.
pub const MAX_HELD_NOTES: usize = 64;

const OFF_HEADROOM: usize = MAX_HELD_NOTES;

/// A validated, playable pattern. Compilation is the trust boundary: past
/// here the audio thread assumes every invariant holds and checks nothing.
#[derive(Clone, Debug)]
pub struct CompiledPattern {
    length_beats: f64,
    root_key: u8,
    follow_after: u8,
    follow_action: FollowAction,
    /// Sorted by start beat. Durations are clamped to the pattern end at
    /// compile, so a note never owes its off to a loop iteration that might
    /// play a different pattern.
    notes: Vec<CompiledNote>,
}

#[derive(Clone, Copy, Debug)]
struct CompiledNote {
    start_beat: f64,
    duration_beats: f64,
    key: u8,
    velocity: u8,
    probability: u8,
    condition: TrigCondition,
    /// The grid tick the note was written on: the deterministic seed of its
    /// probability roll, unmoved by swing.
    seed_tick: u32,
    /// Knobs frozen into this step: (parameter index, value), fired with
    /// the note-on. Fixed-size so the runtime note stays `Copy`.
    locks: [Option<(u32, f64)>; MAX_NOTE_LOCKS],
}

impl CompiledPattern {
    pub fn compile(document: &PatternDefinition) -> Result<Self, PatternError> {
        if document.length_ticks == 0 || document.length_ticks > MAX_PATTERN_TICKS {
            return Err(PatternError::Length);
        }
        if document.notes.len() > MAX_PATTERN_NOTES {
            return Err(PatternError::NoteCount);
        }
        let mut notes = Vec::with_capacity(document.notes.len());
        for note in &document.notes {
            if note.tick >= document.length_ticks {
                return Err(PatternError::NoteOutsidePattern);
            }
            if note.duration_ticks == 0 {
                return Err(PatternError::ZeroDuration);
            }
            if note.key > 127 || note.velocity == 0 || note.velocity > 127 || note.channel > 15 {
                return Err(PatternError::NoteValues);
            }
            // Swing is baked here, at the trust boundary: the grid stays
            // straight in the document, the off-sixteenths land late in the
            // compiled timeline, and the render path never re-computes it.
            let swing =
                f64::from(document.swing_percent.clamp(PATTERN_SWING_STRAIGHT, PATTERN_SWING_MAX));
            let pair = TICKS_PER_BEAT / 2;
            let off = TICKS_PER_BEAT / 4;
            let swung_tick = if note.tick % pair == off {
                f64::from(note.tick - off) + f64::from(pair) * swing / 100.0
            } else {
                f64::from(note.tick)
            };
            let start_beat = swung_tick / f64::from(TICKS_PER_BEAT);
            let end_tick = note.tick.saturating_add(note.duration_ticks).min(document.length_ticks);
            notes.push(CompiledNote {
                start_beat,
                duration_beats: (end_tick - note.tick) as f64 / TICKS_PER_BEAT as f64,
                key: note.key,
                velocity: note.velocity,
                probability: note.probability.clamp(1, 100),
                condition: note.condition,
                seed_tick: note.tick,
                locks: {
                    let mut locks = [None; MAX_NOTE_LOCKS];
                    for (slot, lock) in note.locks.iter().take(MAX_NOTE_LOCKS).enumerate() {
                        locks[slot] = Some((lock.parameter, lock.value));
                    }
                    locks
                },
            });
        }
        notes.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
        Ok(Self {
            length_beats: document.length_ticks as f64 / TICKS_PER_BEAT as f64,
            root_key: document.root_key.min(127),
            follow_after: document.follow_after.min(64),
            follow_action: document.follow_action,
            notes,
        })
    }

    pub fn length_beats(&self) -> f64 {
        self.length_beats
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternError {
    Length,
    NoteCount,
    NoteOutsidePattern,
    ZeroDuration,
    NoteValues,
}

/// A note-off owed to the future.
#[derive(Clone, Copy, Debug)]
struct HeldNote {
    key: u8,
    channel: u8,
    off_beat: f64,
}

/// A pattern armed to begin at an absolute beat. Patterns are shared by
/// `Arc`: the playing copy, the pending copy and the lane's memory are the
/// same compiled data, so re-arming from a PERFORM pad costs a refcount.
struct PendingLaunch {
    pattern: Arc<CompiledPattern>,
    at_beat: f64,
}

/// A pattern currently sounding, anchored at the beat it began.
struct PlayingPattern {
    pattern: Arc<CompiledPattern>,
    anchor_beat: f64,
}

#[derive(Clone, Copy)]
struct StagedEvent {
    frame: u32,
    on: bool,
    data: [u8; 3],
}

/// One lane: one pattern at a time, one MIDI stream out. Routing the stream
/// to an instrument is the integration's business; the lane speaks MIDI only.
pub struct SequencerLane {
    /// The wire channel every event this lane emits is stamped with.
    channel: u8,
    playing: Option<PlayingPattern>,
    pending: Option<PendingLaunch>,
    /// The beat past which no new notes start, when a stop is queued.
    stop_at: Option<f64>,
    /// The lane's variation slots — A/B/C/D — kept across stops the way a
    /// groovebox keeps its clips. Panic is what empties them.
    slots: Vec<Option<Arc<CompiledPattern>>>,
    /// Which slot is the lane's current one: what pads relaunch and what a
    /// plain queue overwrites.
    active_slot: usize,
    /// Key-follow: the phrase sounds only while a key is held, transposed so
    /// its root follows the played note, snapped into this scale.
    follow: Option<SequencerScale>,
    /// Keys currently held on the player's keyboard, in press order; the
    /// last one is the phrase's root of the moment.
    input_keys: Vec<u8>,
    /// A fresh press (from silence) restarts the phrase on the next 16th.
    retrigger_pending: bool,
    /// The last key was released: owed notes are flushed at the next block.
    gate_release_pending: bool,
    /// Whether the most recent conditional trig on this lane fired: what
    /// the pre / not-pre conditions read.
    pre_outcome: bool,
    /// Follow-action jumps taken since launch: the seed of AnySlot's die.
    follow_jumps: u64,
    /// Live capture: armed by the surface whose REC key is down.
    capture: bool,
    /// Notes pressed but not yet released, beat-stamped at the block they
    /// arrived in: (key, velocity, onset beat).
    capture_held: Vec<(u8, u8, f64)>,
    /// Finished notes waiting for the surface to take them.
    captured: Vec<CapturedNoteV1>,
    /// Inputs that arrived since the last block, waiting for a beat stamp.
    capture_inbox: Vec<(u8, u8, bool)>,
    held: Vec<HeldNote>,
    /// Per-block scratch, allocated once here so the render path never does.
    staged: Vec<StagedEvent>,
    muted: bool,
    dropped_notes: u64,
}

impl Default for SequencerLane {
    fn default() -> Self {
        Self::new()
    }
}

impl SequencerLane {
    pub fn new() -> Self {
        Self::with_channel(0)
    }

    /// A lane bound to one wire channel (`0..16`).
    pub fn with_channel(channel: u8) -> Self {
        Self {
            channel: channel & 0x0f,
            playing: None,
            pending: None,
            stop_at: None,
            slots: vec![None; LANE_SLOTS],
            active_slot: 0,
            follow: None,
            input_keys: Vec::with_capacity(10),
            retrigger_pending: false,
            gate_release_pending: false,
            pre_outcome: false,
            follow_jumps: 0,
            capture: false,
            capture_held: Vec::with_capacity(16),
            captured: Vec::with_capacity(64),
            capture_inbox: Vec::with_capacity(32),
            held: Vec::with_capacity(MAX_HELD_NOTES),
            staged: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            muted: false,
            dropped_notes: 0,
        }
    }

    /// Arms `pattern` to begin at `at_beat`, replacing whatever plays then.
    /// A boundary already behind the playhead begins at the next block's
    /// first frame — a stale snapshot must not swallow a launch.
    pub fn queue(&mut self, pattern: CompiledPattern, at_beat: f64) {
        let pattern = Arc::new(pattern);
        self.slots[self.active_slot] = Some(Arc::clone(&pattern));
        self.stop_at = None;
        self.pending = Some(PendingLaunch { pattern, at_beat });
    }

    /// Stores a variation without touching what is sounding.
    pub fn load_slot(&mut self, slot: usize, pattern: CompiledPattern) -> Result<(), String> {
        if slot >= LANE_SLOTS {
            return Err(format!("slot {slot} is outside 0..{LANE_SLOTS}"));
        }
        self.slots[slot] = Some(Arc::new(pattern));
        Ok(())
    }

    /// The A/B/C/D jump: makes `slot` the active variation and queues it.
    pub fn launch_slot(&mut self, slot: usize, at_beat: f64) -> Result<(), String> {
        if slot >= LANE_SLOTS {
            return Err(format!("slot {slot} is outside 0..{LANE_SLOTS}"));
        }
        let Some(pattern) = self.slots[slot].clone() else {
            return Err(format!("slot {slot} holds no pattern"));
        };
        self.active_slot = slot;
        self.stop_at = None;
        self.pending = Some(PendingLaunch { pattern, at_beat });
        Ok(())
    }

    fn armed(&self) -> Option<Arc<CompiledPattern>> {
        self.slots[self.active_slot].clone()
    }

    /// Which slot a follow action lands on. Only loaded slots count; a rack
    /// with no other loaded slot answers the current one, which re-anchors
    /// seamlessly on the boundary — a loop by another name.
    fn follow_target(&self, action: FollowAction) -> Option<usize> {
        let loaded: Vec<usize> = (0..LANE_SLOTS)
            .filter(|&slot| self.slots[slot].is_some())
            .collect();
        if loaded.is_empty() {
            return None;
        }
        match action {
            FollowAction::None | FollowAction::Stop => None,
            FollowAction::NextSlot => (1..=LANE_SLOTS)
                .map(|step| (self.active_slot + step) % LANE_SLOTS)
                .find(|&slot| self.slots[slot].is_some()),
            FollowAction::PreviousSlot => (1..=LANE_SLOTS)
                .map(|step| (self.active_slot + LANE_SLOTS - (step % LANE_SLOTS)) % LANE_SLOTS)
                .find(|&slot| self.slots[slot].is_some()),
            FollowAction::FirstSlot => loaded.first().copied(),
            FollowAction::AnySlot => {
                // The same seeded die as the trig grammar: the jump sequence
                // is a property of the pattern, not of the night.
                let roll = trig_roll(self.follow_jumps, 0xF0110, self.active_slot as u8, self.channel);
                Some(loaded[usize::from(roll) % loaded.len()])
            }
        }
    }

    /// Re-arms the pattern the lane already holds — the PERFORM pad's press.
    /// A lane that holds nothing has nothing to relaunch.
    pub fn relaunch(&mut self, at_beat: f64) -> Result<(), String> {
        let Some(pattern) = self.armed() else {
            return Err("the lane holds no pattern".into());
        };
        self.stop_at = None;
        self.pending = Some(PendingLaunch { pattern, at_beat });
        Ok(())
    }

    /// Whether a stop boundary is set: still sounding, going quiet.
    pub fn is_stopping(&self) -> bool {
        self.stop_at.is_some()
    }

    /// Enters or leaves key-follow. Entering closes the gate until a key
    /// arrives; leaving returns the lane to plain looping.
    pub fn set_follow(&mut self, scale: Option<SequencerScale>) {
        if self.follow.is_some() && scale.is_none() && !self.held.is_empty() {
            // Leaving follow mid-phrase: silence what the gate was holding.
            self.gate_release_pending = true;
        }
        self.follow = scale;
        self.input_keys.clear();
        self.retrigger_pending = false;
    }

    /// Arms or disarms live capture. Disarming abandons unreleased notes.
    pub fn set_capture(&mut self, on: bool) {
        self.capture = on;
        if !on {
            self.capture_held.clear();
            self.capture_inbox.clear();
        }
    }

    /// Everything the lane captured since the last take.
    pub fn capture_take(&mut self) -> Vec<CapturedNoteV1> {
        std::mem::take(&mut self.captured)
    }

    /// One key from the player's keyboard. Follow lanes gate and transpose
    /// on it; an armed lane records it. A press from silence restarts the
    /// phrase; legato presses re-root it without restarting.
    pub fn note_input(&mut self, key: u8, velocity: u8, on: bool) {
        if self.capture && self.capture_inbox.len() < 32 {
            self.capture_inbox.push((key, velocity.max(1), on));
        }
        if self.follow.is_none() {
            return;
        }
        if on {
            if self.input_keys.is_empty() {
                self.retrigger_pending = true;
            }
            self.input_keys.retain(|&held| held != key);
            if self.input_keys.len() < 10 {
                self.input_keys.push(key);
            }
        } else {
            self.input_keys.retain(|&held| held != key);
            if self.input_keys.is_empty() {
                self.gate_release_pending = true;
            }
        }
    }

    /// Stops starting new notes from `at_beat`; owed note-offs still land on
    /// time, so tails ring out instead of clipping at the boundary.
    pub fn stop(&mut self, at_beat: f64) {
        self.pending = None;
        self.stop_at = Some(at_beat);
    }

    /// Whether anything is playing or armed.
    pub fn is_active(&self) -> bool {
        self.playing.is_some() || self.pending.is_some()
    }

    pub fn active_slot(&self) -> usize {
        self.active_slot
    }

    /// A muted lane emits no new note-ons but keeps paying its note-offs:
    /// mute silences the future, never sustains the past.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Note-ons dropped for running out of block or held-note room.
    pub fn dropped_notes(&self) -> u64 {
        self.dropped_notes
    }

    /// Ends every sounding note now but keeps the pattern armed: the flush
    /// a transport stop needs, so silence is silent and start resumes.
    pub fn flush_held_into(&mut self, out: &mut Vec<MidiEventV1>) {
        for held in self.held.drain(..) {
            push_event(out, 0, note_off(held.key, held.channel));
        }
    }

    /// Ends every sounding note now, at the first frame of the next block's
    /// output, and forgets the pattern. The hard stop.
    pub fn panic_into(&mut self, out: &mut Vec<MidiEventV1>) {
        for held in self.held.drain(..) {
            push_event(out, 0, note_off(held.key, held.channel));
        }
        self.playing = None;
        self.pending = None;
        self.stop_at = None;
        self.slots.iter_mut().for_each(|slot| *slot = None);
        self.active_slot = 0;
        self.input_keys.clear();
        self.retrigger_pending = false;
        self.gate_release_pending = false;
        self.pre_outcome = false;
        self.follow_jumps = 0;
        self.capture = false;
        self.capture_held.clear();
        self.captured.clear();
        self.capture_inbox.clear();
    }

    /// Renders one block: pays note-offs falling inside it, executes a
    /// pending launch or stop at its exact frame, and emits the notes of the
    /// playing pattern. Events are appended in frame order, note-offs before
    /// note-ons at the same frame so a repeated key retriggers cleanly.
    ///
    /// `start_beat` and `frames_per_beat` come from the transport's block;
    /// the engine adds nothing to them, which is what keeps the two views of
    /// time from ever disagreeing.
    pub fn render_block(
        &mut self,
        start_beat: f64,
        frames_per_beat: f64,
        frames: u32,
        fill: bool,
        out: &mut Vec<MidiEventV1>,
        params_out: &mut Vec<ParameterEventV1>,
    ) {
        if frames == 0 || frames_per_beat <= 0.0 {
            return;
        }
        let end_beat = start_beat + frames as f64 / frames_per_beat;
        let mut staged = std::mem::take(&mut self.staged);
        staged.clear();

        // 1. Pay the offs owed from earlier blocks first, so the held-note
        // room they release is available to this block's notes.
        pay_due_offs(&mut self.held, end_beat, start_beat, frames_per_beat, frames, &mut staged);

        // Capture bookkeeping: inputs that arrived since the last block are
        // stamped with this block's opening beat — the finest truth the
        // engine has for them — and releases close their notes.
        if self.capture {
            let inbox = std::mem::take(&mut self.capture_inbox);
            for (key, velocity, on) in inbox {
                if on {
                    if self.capture_held.len() < 16 {
                        self.capture_held.push((key, velocity, start_beat));
                    }
                } else if let Some(index) = self
                    .capture_held
                    .iter()
                    .position(|&(held, _, _)| held == key)
                {
                    let (_, velocity, onset) = self.capture_held.swap_remove(index);
                    if self.captured.len() < 256 {
                        self.captured.push(CapturedNoteV1 {
                            beat: onset,
                            key,
                            velocity,
                            duration_beats: (start_beat - onset).max(0.0),
                        });
                    }
                }
            }
        }

        // Key-follow bookkeeping first: a released gate silences what it
        // held, and a fresh press restarts the phrase on the next 16th.
        if self.gate_release_pending {
            self.gate_release_pending = false;
            self.held.retain(|held| {
                staged.push(StagedEvent {
                    frame: 0,
                    on: false,
                    data: note_off(held.key, held.channel),
                });
                let _ = held;
                false
            });
        }
        if self.follow.is_some() && self.retrigger_pending && !self.input_keys.is_empty() {
            if let Some(pattern) = self.armed() {
                self.retrigger_pending = false;
                self.playing = Some(PlayingPattern {
                    pattern,
                    anchor_beat: (start_beat * 4.0).ceil() / 4.0,
                });
            } else {
                self.retrigger_pending = false;
            }
        }

        // The playing pattern's will: after its agreed cycles it names its
        // successor, and the jump lands on the exact cycle boundary through
        // the same pending machinery a queued launch uses. Chains compose:
        // the successor carries its own will.
        if self.pending.is_none()
            && let Some(playing) = &self.playing
            && playing.pattern.follow_after > 0
            && playing.pattern.follow_action != FollowAction::None
        {
            let boundary = playing.anchor_beat
                + f64::from(playing.pattern.follow_after) * playing.pattern.length_beats;
            if boundary < end_beat {
                match playing.pattern.follow_action {
                    FollowAction::Stop => {
                        self.stop_at = Some(boundary.max(start_beat));
                    }
                    action => {
                        if let Some(target) = self.follow_target(action) {
                            self.follow_jumps = self.follow_jumps.wrapping_add(1);
                            self.active_slot = target;
                            if let Some(pattern) = self.slots[target].clone() {
                                self.pending = Some(PendingLaunch {
                                    pattern,
                                    at_beat: boundary,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 2. Execute a pending launch whose boundary this block crosses.
        if self.pending.as_ref().is_some_and(|p| p.at_beat < end_beat) {
            let pending = self.pending.take().expect("pending checked above");
            self.playing = Some(PlayingPattern {
                pattern: pending.pattern,
                anchor_beat: pending.at_beat.max(start_beat),
            });
        }

        // 3. Emit the pattern notes, up to a stop boundary if one arrived.
        let until = match self.stop_at {
            Some(stop_at) if stop_at < end_beat => stop_at.max(start_beat),
            _ => end_beat,
        };
        // A follow lane with no key held is gated silent; with keys held it
        // transposes so the phrase's root is the last-pressed key.
        let transpose = match self.follow {
            None => Some(None),
            Some(scale) => self
                .input_keys
                .last()
                .map(|&played| Some((played, scale))),
        };
        if let (Some(playing), Some(transpose)) = (&self.playing, transpose) {
            emit_pattern_notes(
                playing,
                self.channel,
                self.muted,
                transpose,
                fill,
                &mut self.pre_outcome,
                &mut self.held,
                &mut self.dropped_notes,
                start_beat,
                until,
                frames_per_beat,
                frames,
                &mut staged,
                params_out,
            );
        }
        if until < end_beat {
            self.playing = None;
            self.stop_at = None;
        }

        // 4. A note born in this block may owe its off inside it too — a
        // debt is paid the block it falls due, never the block after.
        pay_due_offs(&mut self.held, end_beat, start_beat, frames_per_beat, frames, &mut staged);

        // 5. Frame order, offs first on ties.
        staged.sort_unstable_by(|a, b| a.frame.cmp(&b.frame).then(a.on.cmp(&b.on)));
        for event in &staged {
            push_event(out, event.frame, event.data);
        }
        self.staged = staged;
    }
}

/// Stages the note-offs falling due before `end_beat` and forgets the debts.
fn pay_due_offs(
    held: &mut Vec<HeldNote>,
    end_beat: f64,
    start_beat: f64,
    frames_per_beat: f64,
    frames: u32,
    staged: &mut Vec<StagedEvent>,
) {
    held.retain(|held| {
        if held.off_beat < end_beat {
            staged.push(StagedEvent {
                frame: frame_of(held.off_beat, start_beat, frames_per_beat, frames),
                on: false,
                data: note_off(held.key, held.channel),
            });
            false
        } else {
            true
        }
    });
}

/// Frame offset of an absolute beat inside the current block.
fn frame_of(beat: f64, start_beat: f64, frames_per_beat: f64, frames: u32) -> u32 {
    let offset = (beat - start_beat).max(0.0) * frames_per_beat;
    (offset as u32).min(frames - 1)
}

/// Emits the playing pattern note-ons inside `[block_start, until)`, looping
/// as many times as the span crosses the pattern boundary. A free function so
/// the lane fields borrow disjointly.
#[allow(clippy::too_many_arguments)]
fn emit_pattern_notes(
    playing: &PlayingPattern,
    channel: u8,
    muted: bool,
    transpose: Option<(u8, SequencerScale)>,
    fill: bool,
    pre_outcome: &mut bool,
    held: &mut Vec<HeldNote>,
    dropped_notes: &mut u64,
    block_start: f64,
    until: f64,
    frames_per_beat: f64,
    frames: u32,
    staged: &mut Vec<StagedEvent>,
    params_out: &mut Vec<ParameterEventV1>,
) {
    // The launch boundary may sit inside the block: nothing sounds before it.
    let from = block_start.max(playing.anchor_beat);
    if until <= from {
        return;
    }
    let length = playing.pattern.length_beats;
    let anchor = playing.anchor_beat;
    // Iterations of the pattern the span touches.
    let first_cycle = ((from - anchor) / length).floor() as u64;
    let last_cycle = (((until - anchor) / length).ceil() as u64).max(first_cycle + 1);
    for cycle in first_cycle..=last_cycle {
        let cycle_start = anchor + cycle as f64 * length;
        if cycle_start >= until {
            break;
        }
        for note in &playing.pattern.notes {
            let at = cycle_start + note.start_beat;
            if at < from {
                continue;
            }
            if at >= until {
                break; // notes are sorted: nothing later in this cycle fits
            }
            // The trig grammar: the condition decides whether this pass may
            // fire, the die decides whether it does. Both deterministic —
            // conditions from the loop count, the die from a seed the show
            // cannot change — so a rehearsal is the gig, roll for roll.
            let permitted = match note.condition {
                TrigCondition::Always => true,
                TrigCondition::Cycle { hit, of } => {
                    cycle % u64::from(of.max(1)) == u64::from(hit.saturating_sub(1))
                }
                TrigCondition::Fill => fill,
                TrigCondition::NotFill => !fill,
                TrigCondition::Pre => *pre_outcome,
                TrigCondition::NotPre => !*pre_outcome,
            };
            let rolled = note.probability >= 100
                || trig_roll(cycle, note.seed_tick, note.key, channel) < note.probability;
            let fires = permitted && rolled;
            if note.condition != TrigCondition::Always || note.probability < 100 {
                *pre_outcome = fires;
            }
            if !fires {
                continue;
            }
            if muted {
                continue;
            }
            // Off headroom is what guarantees a saturated block thins the
            // music instead of sustaining it forever.
            if staged.len() >= MAX_EVENTS_PER_BLOCK - OFF_HEADROOM || held.len() >= MAX_HELD_NOTES {
                *dropped_notes += 1;
                continue;
            }
            let key = match transpose {
                None => note.key,
                Some((played, scale)) => {
                    let offset = i16::from(played) - i16::from(playing.pattern.root_key);
                    match snap_to_scale(i16::from(note.key) + offset, played, scale) {
                        Some(key) => key,
                        // A phrase transposed off the ends of the keyboard
                        // loses those notes rather than folding them.
                        None => continue,
                    }
                }
            };
            let frame = frame_of(at, block_start, frames_per_beat, frames);
            // A step's frozen knobs land with its note-on: same frame, and
            // hosts stage parameters ahead of MIDI, so the sound is already
            // shaped when the note speaks.
            for lock in note.locks.iter().flatten() {
                params_out.push(ParameterEventV1 {
                    frame,
                    parameter_index: lock.0,
                    value: lock.1,
                });
            }
            staged.push(StagedEvent {
                frame,
                on: true,
                data: note_on(key, note.velocity, channel),
            });
            held.push(HeldNote {
                key,
                channel,
                off_beat: at + note.duration_beats,
            });
        }
    }
}

/// The deterministic dice: splitmix64 folded to 0..100. Seeded by the loop
/// pass, the step's grid tick, its key and the lane's channel, so the same
/// pattern rolls the same show every night on every host — and two lanes
/// never share a die.
fn trig_roll(cycle: u64, seed_tick: u32, key: u8, channel: u8) -> u8 {
    let mut x = cycle
        ^ (u64::from(seed_tick) << 32)
        ^ (u64::from(key) << 16)
        ^ (u64::from(channel) << 8)
        ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x % 100) as u8
}

/// The scale's semitone set within one octave of its root.
fn scale_semitones(scale: SequencerScale) -> &'static [u8] {
    match scale {
        SequencerScale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        SequencerScale::Major => &[0, 2, 4, 5, 7, 9, 11],
        SequencerScale::Minor => &[0, 2, 3, 5, 7, 8, 10],
        SequencerScale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
        SequencerScale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
        SequencerScale::PentatonicMajor => &[0, 2, 4, 7, 9],
        SequencerScale::PentatonicMinor => &[0, 3, 5, 7, 10],
    }
}

/// Snaps a (possibly out-of-scale) key into `scale` rooted at `root`,
/// taking the nearest scale tone at or below. Out of MIDI range is `None`:
/// a transposed phrase loses its extremes rather than folding them.
fn snap_to_scale(key: i16, root: u8, scale: SequencerScale) -> Option<u8> {
    if !(0..=127).contains(&key) {
        return None;
    }
    let steps = scale_semitones(scale);
    let rel = (key - i16::from(root)).rem_euclid(12) as u8;
    let snapped_rel = steps
        .iter()
        .rev()
        .find(|&&step| step <= rel)
        .copied()
        .unwrap_or(0);
    let snapped = key - i16::from(rel) + i16::from(snapped_rel);
    (0..=127).contains(&snapped).then_some(snapped as u8)
}

fn note_on(key: u8, velocity: u8, channel: u8) -> [u8; 3] {
    [0x90 | (channel & 0x0f), key, velocity]
}

fn note_off(key: u8, channel: u8) -> [u8; 3] {
    [0x80 | (channel & 0x0f), key, 0x40]
}

fn push_event(out: &mut Vec<MidiEventV1>, frame: u32, data: [u8; 3]) {
    out.push(MidiEventV1 {
        frame,
        length: 3,
        data,
    });
}

/// Variation slots each lane holds: A, B, C, D. Four is the Session-grid
/// convention hardware and software converged on, and it keeps every slot
/// reachable as a single key on a surface.
pub const LANE_SLOTS: usize = 4;

/// Lanes one engine drives. Eight is a hardware-groovebox count: enough for
/// a full live arrangement, small enough that every lane earns a physical
/// control on the surface. The library's Part bindings count the same deck.
pub const MAX_SEQUENCER_LANES: usize = MAX_PART_PATTERN_BINDINGS;

/// Translates one pressed host action — the transport-independent vocabulary
/// a `.rfcontroller` maps its buttons to — into the sequencer command it
/// means, against the host's current state. `TapTempo` translates to nothing
/// here: taps are timestamps, and the timestamps belong to the host's
/// [`TapTempoFold`].
///
/// Every host resolves its buttons through this one function, so PLAY on a
/// KeyLab, PLAY on the web strip and PLAY on any future controller are the
/// same press.
pub fn host_action_command(
    target: rackforge_controller_api::HostActionTarget,
    status: &SequencerStatusV1,
) -> Option<SequencerCommand> {
    use rackforge_controller_api::HostActionTarget as Target;
    match target {
        Target::KeyboardParts | Target::TapTempo => None,
        Target::TransportPlay => Some(SequencerCommand::TransportStart),
        Target::TransportStop => Some(SequencerCommand::TransportStop),
        Target::SequencerLaunchLane { lane } => Some(SequencerCommand::LaunchLane {
            lane,
            quantize: SequencerQuantize::NextBar,
        }),
        Target::SequencerStopLane { lane } => Some(SequencerCommand::StopLane {
            lane,
            quantize: SequencerQuantize::NextBar,
        }),
        Target::SequencerMuteLane { lane } => Some(SequencerCommand::SetLaneMuted {
            lane,
            muted: !status
                .lanes
                .get(usize::from(lane))
                .is_some_and(|state| state.muted),
        }),
        // FILL is momentary and this path only hears presses; a toggle
        // here could strand FILL on. Drivers with both phases (the MCU
        // bridge) dispatch SetFill themselves.
        Target::SequencerFill => None,
    }
}

/// The host's side of tap tempo: it owns the timestamps, the transport owns
/// the arithmetic. Feed it seconds from any monotonic clock; it keeps the
/// last five taps and answers with a tempo once two of them agree.
#[derive(Default)]
pub struct TapTempoFold {
    taps: Vec<f64>,
}

impl TapTempoFold {
    pub fn new() -> Self {
        Self {
            taps: Vec::with_capacity(5),
        }
    }

    pub fn tap(&mut self, now_seconds: f64) -> Option<f64> {
        if self.taps.len() >= 5 {
            self.taps.remove(0);
        }
        self.taps.push(now_seconds);
        crate::transport::tap_tempo(&self.taps)
    }
}

/// The sequencer side of putting a Song Part on stage: the commands that
/// queue each bound pattern on its lane, all at the next bar, so the whole
/// groove of the incoming Part lands together on one boundary.
///
/// This is a pure translation — every host calls it at its own activation
/// site and feeds its own engine. A binding whose pattern has since left the
/// library is skipped rather than failing the activation: the show goes on
/// with the lanes that resolve. Lanes the Part does not bind are left alone;
/// what a musician launched by hand stays theirs across a Part change.
pub fn part_launch_commands(
    part: &SongPart,
    patterns: &[PatternDefinition],
) -> Vec<SequencerCommand> {
    part.patterns
        .iter()
        .filter_map(|binding| {
            let pattern = patterns
                .iter()
                .find(|pattern| pattern.id == binding.pattern_id)?;
            Some(SequencerCommand::QueuePattern {
                lane: binding.lane,
                pattern: pattern.clone(),
                quantize: SequencerQuantize::NextBar,
            })
        })
        .collect()
}

/// The whole sequencer as one host-side object: the transport, the lanes,
/// and the translation from wire commands to sample positions.
///
/// Every host owns exactly one, embedded next to its audio engine, and calls
/// [`SequencerEngine::render_block`] once per block *before* handing MIDI to
/// instances. Commands arrive through [`SequencerEngine::apply`] on whatever
/// thread the host's control channel already crosses; quantise boundaries
/// are resolved here, against this transport, never against a client clock.
pub struct SequencerEngine {
    sample_rate: f64,
    transport: Transport,
    lanes: Vec<SequencerLane>,
    /// The stored pattern names, lane by lane, slot by slot: the engine
    /// compiles patterns namelessly, so the deck's labels live here.
    slot_names: Vec<Vec<Option<String>>>,
    /// A transport stop owes the world silence: pay every held note at the
    /// top of the next block, but keep the patterns armed for resume.
    flush_pending: bool,
    /// A panic owes it a clean slate: notes off and every lane cleared.
    panic_pending: bool,
    /// The FILL performance switch, held by the player.
    fill: bool,
    /// Whether the machine conducts the backline: MIDI clock out.
    clock_enabled: bool,
    /// The next pulse the clock owes, as an absolute 1/24-beat tick index —
    /// an integer, so the grid never drifts by accumulated float error.
    next_clock_tick: u64,
    /// Whether the transport was running when the last block ended — what
    /// turns edges into start/continue/stop bytes.
    clock_was_running: bool,
}

impl SequencerEngine {
    /// A stopped engine at bar one, 120 bpm, four-four.
    pub fn new(sample_rate: f64) -> Option<Self> {
        let transport = Transport::new(sample_rate, 120.0)?;
        Some(Self {
            sample_rate,
            transport,
            lanes: (0..MAX_SEQUENCER_LANES)
                .map(|lane| SequencerLane::with_channel(lane as u8))
                .collect(),
            slot_names: vec![vec![None; LANE_SLOTS]; MAX_SEQUENCER_LANES],
            flush_pending: false,
            panic_pending: false,
            fill: false,
            clock_enabled: false,
            next_clock_tick: 0,
            clock_was_running: false,
        })
    }

    pub fn is_running(&self) -> bool {
        self.transport.is_running()
    }

    /// Applies one wire command. Errors are reporting, not state damage: a
    /// rejected command leaves the engine exactly as it was.
    pub fn apply(&mut self, command: &SequencerCommand) -> Result<(), String> {
        match command {
            SequencerCommand::TransportStart => {
                self.transport.start();
                Ok(())
            }
            SequencerCommand::TransportStop => {
                self.transport.stop();
                self.flush_pending = true;
                Ok(())
            }
            SequencerCommand::TransportPanic => {
                self.transport.stop();
                self.panic_pending = true;
                Ok(())
            }
            SequencerCommand::SetTempo { bpm } => {
                if !bpm.is_finite() {
                    return Err("tempo must be a finite number".into());
                }
                self.transport.set_tempo(*bpm);
                Ok(())
            }
            SequencerCommand::SetSignature {
                beats_per_bar,
                beat_unit,
            } => {
                let signature = TimeSignature::new(*beats_per_bar, *beat_unit)
                    .ok_or_else(|| format!("{beats_per_bar}/{beat_unit} is not a signature"))?;
                self.transport.set_signature(signature);
                Ok(())
            }
            SequencerCommand::QueuePattern {
                lane,
                pattern,
                quantize,
            } => {
                let index = self.lane_index(*lane)?;
                let compiled = CompiledPattern::compile(pattern)
                    .map_err(|error| format!("pattern {:?} rejected: {error:?}", pattern.name))?;
                let at_beat = self.boundary(*quantize);
                self.lanes[index].queue(compiled, at_beat);
                let slot = self.lanes[index].active_slot();
                self.slot_names[index][slot] = Some(pattern.name.clone());
                Ok(())
            }
            SequencerCommand::LoadSlot {
                lane,
                slot,
                pattern,
            } => {
                let index = self.lane_index(*lane)?;
                let compiled = CompiledPattern::compile(pattern)
                    .map_err(|error| format!("pattern {:?} rejected: {error:?}", pattern.name))?;
                self.lanes[index].load_slot(usize::from(*slot), compiled)?;
                self.slot_names[index][usize::from(*slot)] = Some(pattern.name.clone());
                Ok(())
            }
            SequencerCommand::LaunchSlot {
                lane,
                slot,
                quantize,
            } => {
                let index = self.lane_index(*lane)?;
                let at_beat = self.boundary(*quantize);
                self.lanes[index].launch_slot(usize::from(*slot), at_beat)
            }
            SequencerCommand::LaunchLane { lane, quantize } => {
                let index = self.lane_index(*lane)?;
                let at_beat = self.boundary(*quantize);
                self.lanes[index].relaunch(at_beat)
            }
            SequencerCommand::StopLane { lane, quantize } => {
                let index = self.lane_index(*lane)?;
                let at_beat = self.boundary(*quantize);
                self.lanes[index].stop(at_beat);
                Ok(())
            }
            SequencerCommand::SetLaneMuted { lane, muted } => {
                let index = self.lane_index(*lane)?;
                self.lanes[index].set_muted(*muted);
                Ok(())
            }
            SequencerCommand::SetCapture { lane, on } => {
                let index = self.lane_index(*lane)?;
                self.lanes[index].set_capture(*on);
                Ok(())
            }
            SequencerCommand::SetClockOut { on } => {
                self.clock_enabled = *on;
                Ok(())
            }
            SequencerCommand::SetFill { on } => {
                self.fill = *on;
                Ok(())
            }
            SequencerCommand::SetLaneFollow { lane, scale } => {
                let index = self.lane_index(*lane)?;
                self.lanes[index].set_follow(*scale);
                Ok(())
            }
        }
    }

    fn lane_index(&self, lane: u8) -> Result<usize, String> {
        let index = lane as usize;
        if index >= self.lanes.len() {
            return Err(format!("lane {lane} is outside 0..{MAX_SEQUENCER_LANES}"));
        }
        Ok(index)
    }

    /// Resolves a quantise choice to an absolute beat, on this transport.
    fn boundary(&self, quantize: SequencerQuantize) -> f64 {
        match quantize {
            SequencerQuantize::Now => self.transport.position_beats(),
            SequencerQuantize::NextBeat => self.transport.next_beat() as f64,
            SequencerQuantize::NextBar => self.transport.next_bar() as f64,
        }
    }

    /// One key from the player's keyboard, fanned to every follow lane.
    /// Hosts call this from wherever their live MIDI already flows.
    pub fn note_input(&mut self, key: u8, velocity: u8, on: bool) {
        for lane in &mut self.lanes {
            lane.note_input(key, velocity, on);
        }
    }

    /// Drains one lane's capture buffer for the recording surface.
    pub fn capture_take(&mut self, lane: u8) -> Vec<CapturedNoteV1> {
        self.lane_index(lane)
            .map(|index| self.lanes[index].capture_take())
            .unwrap_or_default()
    }

    /// One audio block: debts owed by stops and panics first, then every
    /// lane against the transport's view of the block. Events come out in
    /// frame order, offs before ons on ties, ready for an instance.
    pub fn render_block(
        &mut self,
        frames: u32,
        out: &mut Vec<MidiEventV1>,
        params_out: &mut Vec<ParameterEventV1>,
        clock_out: &mut Vec<MidiEventV1>,
    ) {
        if self.panic_pending {
            self.panic_pending = false;
            self.flush_pending = false;
            for (lane, names) in self.lanes.iter_mut().zip(&mut self.slot_names) {
                lane.panic_into(out);
                names.iter_mut().for_each(|name| *name = None);
            }
        }
        if self.flush_pending {
            self.flush_pending = false;
            for lane in &mut self.lanes {
                lane.flush_held_into(out);
            }
        }
        let block = self.transport.advance(frames);
        let frames_per_beat = self.sample_rate * 60.0 / block.tempo_bpm;
        self.emit_clock(&block, frames, frames_per_beat, clock_out);
        if !block.running {
            return;
        }
        for lane in &mut self.lanes {
            lane.render_block(
                block.start_beat,
                frames_per_beat,
                frames,
                self.fill,
                out,
                params_out,
            );
        }
        // Lanes emitted in sequence; instances expect one timeline. The sort
        // is stable, so each lane's off-before-on ordering survives.
        out.sort_by_key(|event| event.frame);
        params_out.sort_by_key(|event| event.frame);
    }

    /// The conductor's beat: 24 pulses per quarter with exact frame
    /// offsets, plus the start/continue/stop edges the backline expects.
    /// Silent while disabled — and an enable mid-run starts conducting from
    /// the next pulse, no edge byte, the way a chained box would join.
    fn emit_clock(
        &mut self,
        block: &crate::transport::TransportBlock,
        frames: u32,
        frames_per_beat: f64,
        clock_out: &mut Vec<MidiEventV1>,
    ) {
        const TICK: f64 = 1.0 / 24.0;
        let realtime = |frame: u32, byte: u8| MidiEventV1 {
            frame,
            length: 1,
            data: [byte, 0, 0],
        };
        if !self.clock_enabled {
            self.clock_was_running = false;
            return;
        }
        let start_beat = block.start_beat;
        if block.running && !self.clock_was_running {
            // A start edge: 0xFA from the top, 0xFB when resuming mid-song.
            let byte = if start_beat <= f64::EPSILON { 0xfa } else { 0xfb };
            clock_out.push(realtime(0, byte));
            self.next_clock_tick = (start_beat / TICK).ceil() as u64;
        }
        if !block.running && self.clock_was_running {
            clock_out.push(realtime(0, 0xfc));
        }
        self.clock_was_running = block.running;
        if !block.running {
            return;
        }
        let end_beat = start_beat + f64::from(frames) / frames_per_beat;
        loop {
            let pulse_beat = self.next_clock_tick as f64 * TICK;
            if pulse_beat >= end_beat {
                break;
            }
            let offset = ((pulse_beat - start_beat) * frames_per_beat).max(0.0).round() as u32;
            clock_out.push(realtime(offset.min(frames - 1), 0xf8));
            self.next_clock_tick += 1;
        }
    }

    /// What a surface shows. Cheap enough to poll.
    pub fn status(&self) -> SequencerStatusV1 {
        let snapshot = self.transport.snapshot();
        SequencerStatusV1 {
            running: snapshot.running,
            fill: self.fill,
            clock_out: self.clock_enabled,
            tempo_bpm: snapshot.tempo_bpm,
            beats_per_bar: snapshot.signature.beats_per_bar,
            beat_unit: snapshot.signature.beat_unit,
            bar: snapshot.bar,
            beat_in_bar: snapshot.beat_in_bar,
            beat_phase: snapshot.beat_phase,
            lanes: self
                .lanes
                .iter()
                .zip(&self.slot_names)
                .map(|(lane, names)| SequencerLaneStatus {
                    // A follow lane reads as playing only while its gate is
                    // open: the pad tells the truth about what sounds.
                    playing: lane.playing.is_some()
                        && (lane.follow.is_none() || !lane.input_keys.is_empty()),
                    queued: lane.pending.is_some(),
                    stopping: lane.stop_at.is_some(),
                    following: lane.follow.is_some(),
                    capturing: lane.capture,
                    active_slot: lane.active_slot() as u8,
                    slots: names.clone(),
                    muted: lane.muted,
                    pattern_name: names[lane.active_slot()].clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern_id() -> rackforge_performance_api::PatternId {
        rackforge_performance_api::PatternId::new("pattern.test").expect("valid id")
    }

    fn four_on_the_floor() -> CompiledPattern {
        let document = PatternDefinition {
            id: pattern_id(),
            name: "four".into(),
            length_ticks: 4 * TICKS_PER_BEAT,
            notes: (0..4)
                .map(|beat| rackforge_performance_api::PatternNoteSpec {
                    tick: beat * TICKS_PER_BEAT,
                    duration_ticks: TICKS_PER_BEAT / 2,
                    key: 36,
                    velocity: 100,
                    channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
                })
                .collect(),
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
            follow_after: 0,
            follow_action: rackforge_performance_api::FollowAction::None,
        };
        CompiledPattern::compile(&document).expect("valid pattern")
    }

    /// Renders `blocks` blocks of `frames` at 120 bpm / 48 kHz, collecting
    /// `(block, frame, data)` triples the way the audio callback would.
    fn render(
        lane: &mut SequencerLane,
        blocks: usize,
        frames: u32,
        start_beat: f64,
    ) -> Vec<(usize, u32, [u8; 3])> {
        let frames_per_beat = 24_000.0;
        let mut all = Vec::new();
        let mut beat = start_beat;
        for block in 0..blocks {
            let mut out = Vec::with_capacity(MAX_EVENTS_PER_BLOCK);
            lane.render_block(beat, frames_per_beat, frames, false, &mut out, &mut Vec::new());
            for event in out {
                all.push((block, event.frame, event.data));
            }
            beat += frames as f64 / frames_per_beat;
        }
        all
    }

    #[test]
    fn compilation_rejects_malformed_documents() {
        let base = PatternDefinition {
            id: pattern_id(),
            name: "p".into(),
            length_ticks: TICKS_PER_BEAT,
            notes: vec![rackforge_performance_api::PatternNoteSpec {
                tick: 0,
                duration_ticks: 1,
                key: 60,
                velocity: 100,
                channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
            }],
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
            follow_after: 0,
            follow_action: rackforge_performance_api::FollowAction::None,
        };
        let mut zero_length = base.clone();
        zero_length.length_ticks = 0;
        assert_eq!(CompiledPattern::compile(&zero_length).err(), Some(PatternError::Length));
        let mut outside = base.clone();
        outside.notes[0].tick = TICKS_PER_BEAT;
        assert_eq!(
            CompiledPattern::compile(&outside).err(),
            Some(PatternError::NoteOutsidePattern)
        );
        let mut silent = base.clone();
        silent.notes[0].velocity = 0;
        assert_eq!(CompiledPattern::compile(&silent).err(), Some(PatternError::NoteValues));
        let mut still = base.clone();
        still.notes[0].duration_ticks = 0;
        assert_eq!(CompiledPattern::compile(&still).err(), Some(PatternError::ZeroDuration));
        assert!(CompiledPattern::compile(&base).is_ok());
    }

    #[test]
    fn capture_records_what_the_player_did_against_the_transport() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        engine
            .apply(&SequencerCommand::SetCapture { lane: 0, on: true })
            .expect("arm");
        engine.apply(&SequencerCommand::TransportStart).expect("start");
        assert!(engine.status().lanes[0].capturing);

        let mut out = Vec::new();
        let mut params = Vec::new();
        let mut clock = Vec::new();
        // Press C2 before the second half-beat block, release before the
        // fourth: onset at beat 0.5, duration one beat.
        engine.render_block(12_000, &mut out, &mut params, &mut clock);
        engine.note_input(48, 96, true);
        engine.render_block(12_000, &mut out, &mut params, &mut clock);
        engine.render_block(12_000, &mut out, &mut params, &mut clock);
        engine.note_input(48, 0, false);
        engine.render_block(12_000, &mut out, &mut params, &mut clock);

        let take = engine.capture_take(0);
        assert_eq!(take.len(), 1);
        let note = take[0];
        assert_eq!(note.key, 48);
        assert_eq!(note.velocity, 96);
        assert!((note.beat - 0.5).abs() < 1e-9);
        assert!((note.duration_beats - 1.0).abs() < 1e-9);
        // A take drains; disarming clears the held state.
        assert!(engine.capture_take(0).is_empty());
        engine
            .apply(&SequencerCommand::SetCapture { lane: 0, on: false })
            .expect("disarm");
        assert!(!engine.status().lanes[0].capturing);
    }

    #[test]
    fn the_clock_conducts_at_24_ppqn_with_exact_edges() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        engine.apply(&SequencerCommand::SetClockOut { on: true }).expect("sync on");
        let mut out = Vec::new();
        let mut params = Vec::new();
        let mut clock = Vec::new();

        // Silent while stopped.
        engine.render_block(12_000, &mut out, &mut params, &mut clock);
        assert!(clock.is_empty());

        // Start from the top: 0xFA, then a pulse every 1 000 frames at
        // 120 bpm / 48 kHz (24 000 frames per beat / 24).
        engine.apply(&SequencerCommand::TransportStart).expect("start");
        clock.clear();
        engine.render_block(12_000, &mut out, &mut params, &mut clock);
        assert_eq!(clock[0].data[0], 0xfa);
        let pulses: Vec<u32> = clock[1..]
            .iter()
            .map(|event| {
                assert_eq!(event.data[0], 0xf8);
                event.frame
            })
            .collect();
        assert_eq!(pulses, (0..12).map(|tick| tick * 1_000).collect::<Vec<_>>());

        // The next block continues the grid without a seam.
        clock.clear();
        engine.render_block(12_000, &mut out, &mut params, &mut clock);
        let pulses: Vec<u32> = clock.iter().map(|event| event.frame).collect();
        assert_eq!(pulses, (0..12).map(|tick| tick * 1_000).collect::<Vec<_>>());

        // Stop sends its edge once; resuming mid-song is a continue.
        engine.apply(&SequencerCommand::TransportStop).expect("stop");
        clock.clear();
        engine.render_block(12_000, &mut out, &mut params, &mut clock);
        assert_eq!(clock.iter().map(|e| e.data[0]).collect::<Vec<_>>(), vec![0xfc]);
        engine.apply(&SequencerCommand::TransportStart).expect("resume");
        clock.clear();
        engine.render_block(12_000, &mut out, &mut params, &mut clock);
        assert_eq!(clock[0].data[0], 0xfb);
    }

    #[test]
    fn follow_actions_chain_slots_on_the_exact_boundary() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        // A: one C2 per cycle, hands over to the next slot after 2 cycles.
        let mut a = definition();
        a.length_ticks = TICKS_PER_BEAT;
        a.notes = vec![rackforge_performance_api::PatternNoteSpec {
            tick: 0,
            duration_ticks: TICKS_PER_BEAT / 4,
            key: 36,
            velocity: 100,
            channel: 0,
            probability: 100,
            condition: TrigCondition::Always,
            locks: Vec::new(),
        }];
        a.follow_after = 2;
        a.follow_action = FollowAction::NextSlot;
        // B: one D2 per cycle, loops forever.
        let mut b = a.clone();
        b.name = "four-b".into();
        b.notes[0].key = 38;
        b.follow_after = 0;
        b.follow_action = FollowAction::None;

        engine
            .apply(&SequencerCommand::QueuePattern {
                lane: 0,
                pattern: a,
                quantize: SequencerQuantize::Now,
            })
            .expect("queue A");
        engine
            .apply(&SequencerCommand::LoadSlot {
                lane: 0,
                slot: 1,
                pattern: b,
            })
            .expect("load B");
        engine.apply(&SequencerCommand::TransportStart).expect("start");

        // Half-beat blocks (the transport's contract caps block size):
        // cycles 0 and 1 are A, everything after is B, each note on the
        // first frame of its cycle's first block.
        let mut ons = Vec::new();
        let mut out = Vec::new();
        let mut params = Vec::new();
        for block in 0..10 {
            out.clear();
            params.clear();
            engine.render_block(12_000, &mut out, &mut params, &mut Vec::new());
            for event in &out {
                if event.data[0] == 0x90 {
                    ons.push((block, event.data[1], event.frame));
                }
            }
        }
        assert_eq!(
            ons,
            vec![
                (0, 36, 0),
                (2, 36, 0),
                (4, 38, 0),
                (6, 38, 0),
                (8, 38, 0),
            ],
            "the handover lands exactly on the cycle boundary"
        );
        assert_eq!(engine.status().lanes[0].active_slot, 1);
        assert_eq!(engine.status().lanes[0].pattern_name.as_deref(), Some("four-b"));
    }

    #[test]
    fn parameter_locks_fire_with_their_note() {
        let mut document = definition();
        document.length_ticks = TICKS_PER_BEAT;
        document.notes = vec![rackforge_performance_api::PatternNoteSpec {
            tick: TICKS_PER_BEAT / 2,
            duration_ticks: TICKS_PER_BEAT / 4,
            key: 48,
            velocity: 100,
            channel: 0,
            probability: 100,
            condition: TrigCondition::Always,
            locks: vec![
                rackforge_performance_api::ParameterLockSpec {
                    parameter: 3,
                    value: 0.42,
                },
                rackforge_performance_api::ParameterLockSpec {
                    parameter: 7,
                    value: 12_000.0,
                },
            ],
        }];
        let mut lane = SequencerLane::new();
        lane.queue(
            CompiledPattern::compile(&document).expect("valid pattern"),
            0.0,
        );
        let mut out = Vec::new();
        let mut params = Vec::new();
        lane.render_block(0.0, 24_000.0, 24_000, false, &mut out, &mut params);
        let note_frame = out
            .iter()
            .find(|event| event.data[0] == 0x90)
            .expect("the note fires")
            .frame;
        assert_eq!(note_frame, 12_000);
        // Both frozen knobs land on the note's exact frame, values intact.
        assert_eq!(params.len(), 2);
        assert!(params.iter().all(|event| event.frame == note_frame));
        assert!(params.iter().any(|e| e.parameter_index == 3 && (e.value - 0.42).abs() < 1e-12));
        assert!(params.iter().any(|e| e.parameter_index == 7 && (e.value - 12_000.0).abs() < 1e-9));

        // A skipped step keeps its knobs frozen: no note, no lock.
        let mut chance = document.clone();
        chance.notes[0].condition = TrigCondition::Fill;
        let mut lane = SequencerLane::new();
        lane.queue(CompiledPattern::compile(&chance).expect("valid"), 0.0);
        let mut out = Vec::new();
        let mut params = Vec::new();
        lane.render_block(0.0, 24_000.0, 24_000, false, &mut out, &mut params);
        assert!(out.is_empty() && params.is_empty());
    }

    #[test]
    fn slots_hold_four_variations_and_switch_on_the_bar() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        let mut variation = definition();
        variation.name = "four-b".into();
        variation.notes.truncate(1);
        engine
            .apply(&SequencerCommand::QueuePattern {
                lane: 0,
                pattern: definition(),
                quantize: SequencerQuantize::Now,
            })
            .expect("queue into slot A");
        engine
            .apply(&SequencerCommand::LoadSlot {
                lane: 0,
                slot: 1,
                pattern: variation,
            })
            .expect("load slot B");

        let status = engine.status();
        assert_eq!(status.lanes[0].active_slot, 0);
        assert_eq!(
            status.lanes[0].slots,
            vec![Some("four".into()), Some("four-b".into()), None, None]
        );
        assert_eq!(status.lanes[0].pattern_name.as_deref(), Some("four"));

        // The B jump: active slot moves, the pad's relaunch target with it.
        engine.apply(&SequencerCommand::TransportStart).expect("start");
        engine
            .apply(&SequencerCommand::LaunchSlot {
                lane: 0,
                slot: 1,
                quantize: SequencerQuantize::Now,
            })
            .expect("launch B");
        let status = engine.status();
        assert_eq!(status.lanes[0].active_slot, 1);
        assert_eq!(status.lanes[0].pattern_name.as_deref(), Some("four-b"));

        // An empty slot refuses the jump and changes nothing.
        assert!(
            engine
                .apply(&SequencerCommand::LaunchSlot {
                    lane: 0,
                    slot: 3,
                    quantize: SequencerQuantize::Now,
                })
                .is_err()
        );
        assert_eq!(engine.status().lanes[0].active_slot, 1);
    }

    #[test]
    fn conditions_and_probability_are_the_same_show_every_night() {
        // Four cycles of a 1-beat pattern with one note per grammar case.
        let mut document = definition();
        document.length_ticks = TICKS_PER_BEAT;
        document.notes = vec![
            rackforge_performance_api::PatternNoteSpec {
                tick: 0,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 36, // always
                velocity: 100,
                channel: 0,
                probability: 100,
                condition: TrigCondition::Always,
                locks: Vec::new(),
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: TICKS_PER_BEAT / 4,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 38, // second pass of every two
                velocity: 100,
                channel: 0,
                probability: 100,
                condition: TrigCondition::Cycle { hit: 2, of: 2 },
                locks: Vec::new(),
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: TICKS_PER_BEAT / 2,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 42, // echoes the cycle note via pre
                velocity: 100,
                channel: 0,
                probability: 100,
                condition: TrigCondition::Pre,
                locks: Vec::new(),
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: 3 * TICKS_PER_BEAT / 4,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 46, // fill only
                velocity: 100,
                channel: 0,
                probability: 100,
                condition: TrigCondition::Fill,
                locks: Vec::new(),
            },
        ];
        let run = |fill: bool| {
            let mut lane = SequencerLane::new();
            lane.queue(
                CompiledPattern::compile(&document).expect("valid pattern"),
                0.0,
            );
            let frames_per_beat = 24_000.0;
            let mut all = Vec::new();
            let mut beat = 0.0;
            for cycle in 0..4 {
                let mut out = Vec::new();
                lane.render_block(beat, frames_per_beat, 24_000, fill, &mut out, &mut Vec::new());
                for event in out {
                    if event.data[0] == 0x90 {
                        all.push((cycle, event.data[1]));
                    }
                }
                beat += 1.0;
            }
            all
        };
        let quiet = run(false);
        // Kick every pass; snare on passes 1 and 3 (2:2); the pre note
        // echoes the snare's outcome; no fill note.
        assert_eq!(
            quiet,
            vec![
                (0, 36),
                (1, 36),
                (1, 38),
                (1, 42),
                (2, 36),
                (3, 36),
                (3, 38),
                (3, 42),
            ]
        );
        // Determinism: the same show, note for note.
        assert_eq!(quiet, run(false));
        // FILL adds exactly the fill-gated note to every pass.
        let loud = run(true);
        assert_eq!(loud.iter().filter(|(_, key)| *key == 46).count(), 4);

        // Probability rolls are deterministic and honour the odds shape:
        // the same seeded die twice, and a 50 that neither always nor
        // never fires across many cycles.
        let mut chance = definition();
        chance.length_ticks = TICKS_PER_BEAT;
        chance.notes = vec![rackforge_performance_api::PatternNoteSpec {
            tick: 0,
            duration_ticks: TICKS_PER_BEAT / 4,
            key: 36,
            velocity: 100,
            channel: 0,
            probability: 50,
            condition: TrigCondition::Always,
                locks: Vec::new(),
        }];
        let roll_run = || {
            let mut lane = SequencerLane::new();
            lane.queue(CompiledPattern::compile(&chance).expect("valid"), 0.0);
            let mut fired = Vec::new();
            let mut beat = 0.0;
            for cycle in 0..32 {
                let mut out = Vec::new();
                lane.render_block(beat, 24_000.0, 24_000, false, &mut out, &mut Vec::new());
                if out.iter().any(|event| event.data[0] == 0x90) {
                    fired.push(cycle);
                }
                beat += 1.0;
            }
            fired
        };
        let first = roll_run();
        assert_eq!(first, roll_run(), "the die must be seeded, not random");
        assert!(!first.is_empty() && first.len() < 32, "a 50 is neither 0 nor 100");
    }

    #[test]
    fn key_follow_gates_transposes_and_snaps() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        // A one-note phrase at its root, C2, on a 1-beat pattern.
        let mut document = definition();
        document.length_ticks = TICKS_PER_BEAT;
        document.notes = vec![rackforge_performance_api::PatternNoteSpec {
            // A full-beat note, so something is still ringing at release.
            tick: 0,
            duration_ticks: TICKS_PER_BEAT,
            key: 48,
            velocity: 100,
            channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
        }];
        engine
            .apply(&SequencerCommand::QueuePattern {
                lane: 0,
                pattern: document,
                quantize: SequencerQuantize::Now,
            })
            .expect("queue");
        engine
            .apply(&SequencerCommand::SetLaneFollow {
                lane: 0,
                scale: Some(SequencerScale::Chromatic),
            })
            .expect("follow");
        engine.apply(&SequencerCommand::TransportStart).expect("start");

        // Gate closed: a full bar of silence.
        let mut out = Vec::new();
        engine.render_block(16_384, &mut out, &mut Vec::new(), &mut Vec::new());
        assert!(out.is_empty(), "a follow lane must be silent with no key held");
        assert!(engine.status().lanes[0].following);
        assert!(!engine.status().lanes[0].playing);

        // Hold F2: the phrase restarts on the next 16th, five semitones up.
        engine.note_input(53, 100, true);
        out.clear();
        engine.render_block(16_384, &mut out, &mut Vec::new(), &mut Vec::new());
        let ons: Vec<u8> = out
            .iter()
            .filter(|event| event.data[0] == 0x90)
            .map(|event| event.data[1])
            .collect();
        assert!(ons.iter().all(|&key| key == 53), "expected F2, got {ons:?}");
        assert!(!ons.is_empty());
        assert!(engine.status().lanes[0].playing);

        // Release: the ringing note is silenced at the next block.
        engine.note_input(53, 0, false);
        out.clear();
        engine.render_block(16_384, &mut out, &mut Vec::new(), &mut Vec::new());
        assert!(out.iter().any(|event| event.data[0] == 0x80));
        assert!(out.iter().all(|event| event.data[0] != 0x90));

        // Scale snap: a phrase note a minor third over the root, held on F2
        // in F major, snaps 56 (G#) down to 55 (G).
        assert_eq!(snap_to_scale(48 + 5 + 3, 53, SequencerScale::Major), Some(55));
        assert_eq!(snap_to_scale(200, 53, SequencerScale::Major), None);
    }

    #[test]
    fn swing_moves_the_off_sixteenths_and_nothing_else() {
        let mut document = definition();
        document.length_ticks = TICKS_PER_BEAT;
        document.notes = vec![
            rackforge_performance_api::PatternNoteSpec {
                tick: 0,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 36,
                velocity: 100,
                channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: TICKS_PER_BEAT / 4, // the off-sixteenth
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 38,
                velocity: 100,
                channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: TICKS_PER_BEAT / 2, // the next pair's downbeat
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 42,
                velocity: 100,
                channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
            },
        ];
        document.swing_percent = 66;
        let mut lane = SequencerLane::new();
        lane.queue(
            CompiledPattern::compile(&document).expect("valid pattern"),
            0.0,
        );
        let events = render(&mut lane, 1, 24_000, 0.0);
        let ons: Vec<(u32, u8)> = events
            .iter()
            .filter(|(_, _, data)| data[0] == 0x90)
            .map(|(_, frame, data)| (*frame, data[1]))
            .collect();
        // Downbeats hold their frames; the off-sixteenth lands at 66% of
        // its eighth-note pair: 0.33 beats -> frame 7 920, not 6 000.
        assert_eq!(ons, vec![(0, 36), (7_920, 38), (12_000, 42)]);
    }

    #[test]
    fn a_launch_lands_on_its_exact_frame() {
        let mut lane = SequencerLane::new();
        // Launch at beat 2 while rendering blocks of half a beat.
        lane.queue(four_on_the_floor(), 2.0);
        let events = render(&mut lane, 8, 12_000, 0.0);
        // First event: note-on at block 4, frame 0 (beat 2 exactly).
        assert_eq!(events[0], (4, 0, [0x90, 36, 100]));
    }

    #[test]
    fn a_mid_block_launch_offsets_into_the_block() {
        let mut lane = SequencerLane::new();
        lane.queue(four_on_the_floor(), 0.25);
        let events = render(&mut lane, 2, 24_000, 0.0);
        // Beat 0.25 at 24 000 frames per beat: frame 6 000 of block 0.
        assert_eq!(events[0], (0, 6_000, [0x90, 36, 100]));
    }

    #[test]
    fn note_offs_pay_their_debts_on_time() {
        let mut lane = SequencerLane::new();
        lane.queue(four_on_the_floor(), 0.0);
        let events = render(&mut lane, 4, 24_000, 0.0);
        // Each half-beat note offs 12 000 frames after it starts.
        assert!(events.contains(&(0, 0, [0x90, 36, 100])));
        assert!(events.contains(&(0, 12_000, [0x80, 36, 0x40])));
        assert!(events.contains(&(1, 0, [0x90, 36, 100])));
        assert!(events.contains(&(1, 12_000, [0x80, 36, 0x40])));
    }

    #[test]
    fn the_pattern_loops_seamlessly() {
        let mut lane = SequencerLane::new();
        lane.queue(four_on_the_floor(), 0.0);
        // 6 beats of audio: the pattern (4 beats) wraps into its second pass.
        let events = render(&mut lane, 6, 24_000, 0.0);
        let ons: Vec<usize> = events
            .iter()
            .filter(|(_, _, data)| data[0] == 0x90)
            .map(|(block, _, _)| *block)
            .collect();
        assert_eq!(ons, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn switching_patterns_is_seamless_and_pays_old_offs() {
        let mut lane = SequencerLane::new();
        lane.queue(four_on_the_floor(), 0.0);
        render(&mut lane, 1, 24_000, 0.0); // sound beat 0; its off is owed
        // A different pattern takes over at beat 1.
        let document = PatternDefinition {
            id: pattern_id(),
            name: "high".into(),
            length_ticks: TICKS_PER_BEAT,
            notes: vec![rackforge_performance_api::PatternNoteSpec {
                tick: 0,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 60,
                velocity: 90,
                channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
            }],
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
            follow_after: 0,
            follow_action: rackforge_performance_api::FollowAction::None,
        };
        lane.queue(CompiledPattern::compile(&document).expect("valid"), 1.0);
        let events = render(&mut lane, 1, 24_000, 1.0);
        // The new pattern starts exactly on the boundary and the old one
        // does not play past it.
        assert_eq!(events[0], (0, 0, [0x90, 60, 90]));
        assert!(
            !events.iter().any(|(_, _, data)| data == &[0x90, 36, 100]),
            "the replaced pattern kept playing past its boundary"
        );
    }

    #[test]
    fn stop_lets_tails_ring_and_panic_does_not() {
        let mut lane = SequencerLane::new();
        lane.queue(four_on_the_floor(), 0.0);
        render(&mut lane, 1, 12_000, 0.0); // note on, off still owed
        lane.stop(0.5);
        let events = render(&mut lane, 1, 12_000, 0.5);
        // The stop did not clip the sounding note: its off lands on schedule.
        assert_eq!(events, vec![(0, 0, [0x80, 36, 0x40])]);
        assert!(!lane.is_active());

        let mut lane = SequencerLane::new();
        lane.queue(four_on_the_floor(), 0.0);
        render(&mut lane, 1, 6_000, 0.0); // note still sounding
        let mut out = Vec::new();
        lane.panic_into(&mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data, [0x80, 36, 0x40]);
        assert_eq!(out[0].frame, 0);
    }

    #[test]
    fn mute_silences_the_future_but_pays_the_past() {
        let mut lane = SequencerLane::new();
        lane.queue(four_on_the_floor(), 0.0);
        render(&mut lane, 1, 12_000, 0.0); // beat 0 sounded
        lane.set_muted(true);
        let events = render(&mut lane, 3, 24_000, 0.5);
        // No new ons, but beat 0's off still arrives.
        assert!(events.iter().all(|(_, _, data)| data[0] == 0x80));
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn offs_precede_ons_on_the_same_frame() {
        // A one-beat note repeated back to back: the off of pass N and the on
        // of pass N+1 share a frame, and the off must come first.
        let document = PatternDefinition {
            id: pattern_id(),
            name: "legato".into(),
            length_ticks: TICKS_PER_BEAT,
            notes: vec![rackforge_performance_api::PatternNoteSpec {
                tick: 0,
                duration_ticks: TICKS_PER_BEAT,
                key: 60,
                velocity: 100,
                channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
            }],
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
            follow_after: 0,
            follow_action: rackforge_performance_api::FollowAction::None,
        };
        let mut lane = SequencerLane::new();
        lane.queue(CompiledPattern::compile(&document).expect("valid"), 0.0);
        let events = render(&mut lane, 2, 24_000, 0.0);
        let block1: Vec<&[u8; 3]> = events
            .iter()
            .filter(|(block, frame, _)| *block == 1 && *frame == 0)
            .map(|(_, _, data)| data)
            .collect();
        assert_eq!(block1, vec![&[0x80, 60, 0x40], &[0x90, 60, 100]]);
    }

    #[test]
    fn saturation_drops_ons_and_never_offs() {
        // A pathological pattern: hundreds of simultaneous notes.
        let document = PatternDefinition {
            id: pattern_id(),
            name: "wall".into(),
            length_ticks: TICKS_PER_BEAT,
            notes: (0..127)
                .map(|key| rackforge_performance_api::PatternNoteSpec {
                    tick: 0,
                    duration_ticks: TICKS_PER_BEAT / 2,
                    key,
                    velocity: 100,
                    channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
                })
                .collect(),
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
            follow_after: 0,
            follow_action: rackforge_performance_api::FollowAction::None,
        };
        let mut lane = SequencerLane::new();
        lane.queue(CompiledPattern::compile(&document).expect("valid"), 0.0);
        let events = render(&mut lane, 2, 24_000, 0.0);
        let ons = events.iter().filter(|(_, _, d)| d[0] == 0x90).count();
        let offs = events.iter().filter(|(_, _, d)| d[0] == 0x80).count();
        // Held-note room capped the chord, every sounded note got its off,
        // and the drops were counted.
        assert_eq!(ons, MAX_HELD_NOTES * 2);
        assert_eq!(offs, ons);
        assert!(lane.dropped_notes() > 0);
    }


    #[test]
    fn rendering_is_deterministic() {
        let run = || {
            let mut lane = SequencerLane::new();
            lane.queue(four_on_the_floor(), 0.75);
            let mut events = render(&mut lane, 12, 4_096, 0.0);
            lane.set_muted(true);
            events.extend(render(&mut lane, 4, 4_096, 12.0 * 4_096.0 / 24_000.0));
            events
        };
        assert_eq!(run(), run());
    }

    fn definition() -> PatternDefinition {
        PatternDefinition {
            id: pattern_id(),
            name: "four".into(),
            length_ticks: 4 * TICKS_PER_BEAT,
            notes: (0..4)
                .map(|beat| rackforge_performance_api::PatternNoteSpec {
                    tick: beat * TICKS_PER_BEAT,
                    duration_ticks: TICKS_PER_BEAT / 2,
                    key: 36,
                    velocity: 100,
                    channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
                    locks: Vec::new(),
                })
                .collect(),
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
            follow_after: 0,
            follow_action: rackforge_performance_api::FollowAction::None,
        }
    }

    #[test]
    fn the_engine_queues_on_the_bar_and_stop_flushes_silence() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        engine
            .apply(&SequencerCommand::QueuePattern {
                lane: 0,
                pattern: definition(),
                quantize: SequencerQuantize::NextBar,
            })
            .expect("queue accepted");
        engine.apply(&SequencerCommand::TransportStart).expect("start");
        let mut out = Vec::new();
        engine.render_block(4_096, &mut out, &mut Vec::new(), &mut Vec::new());
        // At bar one beat zero the pattern begins on frame 0.
        assert_eq!(out[0].data, [0x90, 36, 100]);
        assert_eq!(out[0].frame, 0);
        assert!(engine.status().running);
        assert!(engine.status().lanes[0].playing);

        // Stop: silence is owed at the top of the next block, the pattern
        // stays armed.
        engine.apply(&SequencerCommand::TransportStop).expect("stop");
        out.clear();
        engine.render_block(4_096, &mut out, &mut Vec::new(), &mut Vec::new());
        assert!(out.iter().all(|event| event.data[0] == 0x80));
        assert!(!out.is_empty(), "the sounding note was left ringing");
        assert!(engine.status().lanes[0].playing, "stop must not clear the lane");

        // Panic instead clears everything.
        engine.apply(&SequencerCommand::TransportPanic).expect("panic");
        out.clear();
        engine.render_block(4_096, &mut out, &mut Vec::new(), &mut Vec::new());
        assert!(!engine.status().lanes[0].playing);
        assert_eq!(engine.status().lanes[0].pattern_name, None);
    }

    #[test]
    fn host_actions_translate_against_current_state() {
        use rackforge_controller_api::HostActionTarget as Target;
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        engine
            .apply(&SequencerCommand::QueuePattern {
                lane: 1,
                pattern: definition(),
                quantize: SequencerQuantize::Now,
            })
            .expect("queue accepted");

        let status = engine.status();
        assert_eq!(
            host_action_command(Target::TransportPlay, &status),
            Some(SequencerCommand::TransportStart)
        );
        assert_eq!(
            host_action_command(Target::SequencerLaunchLane { lane: 1 }, &status),
            Some(SequencerCommand::LaunchLane {
                lane: 1,
                quantize: SequencerQuantize::NextBar,
            })
        );
        // Mute is a toggle against what the host reports right now.
        assert_eq!(
            host_action_command(Target::SequencerMuteLane { lane: 1 }, &status),
            Some(SequencerCommand::SetLaneMuted { lane: 1, muted: true })
        );
        engine
            .apply(&SequencerCommand::SetLaneMuted { lane: 1, muted: true })
            .expect("mute");
        assert_eq!(
            host_action_command(Target::SequencerMuteLane { lane: 1 }, &engine.status()),
            Some(SequencerCommand::SetLaneMuted { lane: 1, muted: false })
        );
        // Taps are the host's business, not a command.
        assert_eq!(host_action_command(Target::TapTempo, &status), None);

        let mut fold = TapTempoFold::new();
        assert_eq!(fold.tap(0.0), None);
        let bpm = fold.tap(0.5).expect("two taps make a tempo");
        assert!((bpm - 120.0).abs() < 1e-6);
    }

    #[test]
    fn a_part_activation_queues_what_it_binds_and_skips_what_left() {
        use rackforge_performance_api::{
            PatternId, RackId, SongPartId, SongPartPatternBinding,
        };
        let library_patterns = vec![definition()];
        let part = SongPart {
            id: SongPartId::new("part.chorus").expect("valid id"),
            name: "Chorus".into(),
            rack_id: RackId::new("rack.main").expect("valid id"),
            content: None,
            patterns: vec![
                SongPartPatternBinding {
                    lane: 2,
                    pattern_id: library_patterns[0].id.clone(),
                },
                SongPartPatternBinding {
                    lane: 5,
                    pattern_id: PatternId::new("pattern.deleted").expect("valid id"),
                },
            ],
        };
        let commands = part_launch_commands(&part, &library_patterns);
        // The resolvable binding queues on its lane at the bar; the stale
        // one is skipped — the show goes on.
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            SequencerCommand::QueuePattern {
                lane,
                pattern,
                quantize,
            } => {
                assert_eq!(*lane, 2);
                assert_eq!(pattern.name, "four");
                assert_eq!(*quantize, SequencerQuantize::NextBar);
            }
            other => panic!("unexpected command {other:?}"),
        }

        // And the commands drive the engine as-is: lane 2 ends up armed.
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        for command in &commands {
            engine.apply(command).expect("part command accepted");
        }
        assert!(engine.status().lanes[2].queued);
        assert_eq!(engine.status().lanes[2].pattern_name.as_deref(), Some("four"));
    }

    #[test]
    fn a_pad_relaunches_what_the_lane_remembers() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        engine
            .apply(&SequencerCommand::QueuePattern {
                lane: 0,
                pattern: definition(),
                quantize: SequencerQuantize::Now,
            })
            .expect("queue accepted");
        engine.apply(&SequencerCommand::TransportStart).expect("start");
        let mut out = Vec::new();
        engine.render_block(16_384, &mut out, &mut Vec::new(), &mut Vec::new());
        assert!(out.iter().any(|event| event.data[0] == 0x90));

        // Stop the lane; while the boundary is pending the status says so.
        engine
            .apply(&SequencerCommand::StopLane {
                lane: 0,
                quantize: SequencerQuantize::NextBar,
            })
            .expect("stop accepted");
        assert!(engine.status().lanes[0].stopping);
        // Run past the bar: the lane is silent but still remembers.
        for _ in 0..16 {
            out.clear();
            engine.render_block(16_384, &mut out, &mut Vec::new(), &mut Vec::new());
        }
        let status = engine.status();
        assert!(!status.lanes[0].playing);
        assert!(!status.lanes[0].stopping);
        assert_eq!(status.lanes[0].pattern_name.as_deref(), Some("four"));

        // The pad press: no document travels, the lane re-arms itself.
        engine
            .apply(&SequencerCommand::LaunchLane {
                lane: 0,
                quantize: SequencerQuantize::NextBar,
            })
            .expect("relaunch accepted");
        assert!(engine.status().lanes[0].queued);
        let mut sounded = false;
        for _ in 0..16 {
            out.clear();
            engine.render_block(16_384, &mut out, &mut Vec::new(), &mut Vec::new());
            sounded |= out.iter().any(|event| event.data[0] == 0x90);
        }
        assert!(sounded, "the relaunched lane never played");

        // An empty lane refuses the pad, and panic empties the memory.
        assert!(
            engine
                .apply(&SequencerCommand::LaunchLane {
                    lane: 3,
                    quantize: SequencerQuantize::Now,
                })
                .is_err()
        );
        engine.apply(&SequencerCommand::TransportPanic).expect("panic");
        out.clear();
        engine.render_block(16_384, &mut out, &mut Vec::new(), &mut Vec::new());
        assert!(
            engine
                .apply(&SequencerCommand::LaunchLane {
                    lane: 0,
                    quantize: SequencerQuantize::Now,
                })
                .is_err(),
            "panic must empty every lane's memory"
        );
    }

    #[test]
    fn each_lane_speaks_on_its_own_channel() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        engine
            .apply(&SequencerCommand::QueuePattern {
                lane: 2,
                pattern: definition(),
                quantize: SequencerQuantize::Now,
            })
            .expect("queue accepted");
        engine.apply(&SequencerCommand::TransportStart).expect("start");
        let mut out = Vec::new();
        engine.render_block(16_384, &mut out, &mut Vec::new(), &mut Vec::new());
        assert!(!out.is_empty());
        // Lane 2 emits on wire channel 2 — ons and offs alike — which a Rack
        // Slot filters as musician-facing channel 3.
        assert!(out.iter().all(|event| event.data[0] & 0x0f == 2));
        assert!(out.iter().any(|event| event.data[0] & 0xf0 == 0x80));
    }

    #[test]
    fn the_engine_rejects_what_it_should() {
        let mut engine = SequencerEngine::new(48_000.0).expect("engine");
        assert!(
            engine
                .apply(&SequencerCommand::QueuePattern {
                    lane: MAX_SEQUENCER_LANES as u8,
                    pattern: definition(),
                    quantize: SequencerQuantize::Now,
                })
                .is_err()
        );
        assert!(
            engine
                .apply(&SequencerCommand::SetSignature { beats_per_bar: 0, beat_unit: 4 })
                .is_err()
        );
        assert!(engine.apply(&SequencerCommand::SetTempo { bpm: f64::NAN }).is_err());
        // Out-of-range tempo clamps rather than failing: a torn tap on stage
        // must never leave the old tempo standing.
        engine.apply(&SequencerCommand::SetTempo { bpm: 9_000.0 }).expect("clamped");
        assert!(engine.status().tempo_bpm <= 400.0);
    }

    #[test]
    fn a_stale_launch_boundary_fires_immediately() {
        let mut lane = SequencerLane::new();
        // Queued for beat 1, but the playhead is already at beat 3.
        lane.queue(four_on_the_floor(), 1.0);
        let events = render(&mut lane, 1, 24_000, 3.0);
        // The pattern anchors at the block start rather than being swallowed.
        assert_eq!(events[0], (0, 0, [0x90, 36, 100]));
    }
}
