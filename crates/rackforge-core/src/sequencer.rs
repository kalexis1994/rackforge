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
    SequencerCommand, SequencerLaneStatus, SequencerQuantize, SequencerScale, SequencerStatusV1,
};
use rackforge_performance_api::{
    MAX_PART_PATTERN_BINDINGS, MAX_PATTERN_NOTES, MAX_PATTERN_TICKS, PATTERN_SWING_MAX,
    PATTERN_SWING_STRAIGHT, PATTERN_TICKS_PER_BEAT, PatternDefinition, SongPart, TrigCondition,
};
use rackforge_plugin_api::abi::MidiEventV1;
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
            });
        }
        notes.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
        Ok(Self {
            length_beats: document.length_ticks as f64 / TICKS_PER_BEAT as f64,
            root_key: document.root_key.min(127),
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
    /// The pattern this lane holds, kept across stops the way a groovebox
    /// track keeps its clip. Panic is what empties a lane.
    armed: Option<Arc<CompiledPattern>>,
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
            armed: None,
            follow: None,
            input_keys: Vec::with_capacity(10),
            retrigger_pending: false,
            gate_release_pending: false,
            pre_outcome: false,
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
        self.armed = Some(Arc::clone(&pattern));
        self.stop_at = None;
        self.pending = Some(PendingLaunch { pattern, at_beat });
    }

    /// Re-arms the pattern the lane already holds — the PERFORM pad's press.
    /// A lane that holds nothing has nothing to relaunch.
    pub fn relaunch(&mut self, at_beat: f64) -> Result<(), String> {
        let Some(pattern) = self.armed.clone() else {
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

    /// One key from the player's keyboard. Only follow lanes listen. A press
    /// from silence restarts the phrase; legato presses re-root it without
    /// restarting — the classic monosynth-sequencer feel.
    pub fn note_input(&mut self, key: u8, on: bool) {
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
        self.armed = None;
        self.input_keys.clear();
        self.retrigger_pending = false;
        self.gate_release_pending = false;
        self.pre_outcome = false;
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
            if let Some(pattern) = self.armed.clone() {
                self.retrigger_pending = false;
                self.playing = Some(PlayingPattern {
                    pattern,
                    anchor_beat: (start_beat * 4.0).ceil() / 4.0,
                });
            } else {
                self.retrigger_pending = false;
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
            staged.push(StagedEvent {
                frame: frame_of(at, block_start, frames_per_beat, frames),
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
    lane_names: Vec<Option<String>>,
    /// A transport stop owes the world silence: pay every held note at the
    /// top of the next block, but keep the patterns armed for resume.
    flush_pending: bool,
    /// A panic owes it a clean slate: notes off and every lane cleared.
    panic_pending: bool,
    /// The FILL performance switch, held by the player.
    fill: bool,
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
            lane_names: vec![None; MAX_SEQUENCER_LANES],
            flush_pending: false,
            panic_pending: false,
            fill: false,
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
                self.lane_names[index] = Some(pattern.name.clone());
                Ok(())
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
    pub fn note_input(&mut self, key: u8, on: bool) {
        for lane in &mut self.lanes {
            lane.note_input(key, on);
        }
    }

    /// One audio block: debts owed by stops and panics first, then every
    /// lane against the transport's view of the block. Events come out in
    /// frame order, offs before ons on ties, ready for an instance.
    pub fn render_block(&mut self, frames: u32, out: &mut Vec<MidiEventV1>) {
        if self.panic_pending {
            self.panic_pending = false;
            self.flush_pending = false;
            for (lane, name) in self.lanes.iter_mut().zip(&mut self.lane_names) {
                lane.panic_into(out);
                *name = None;
            }
        }
        if self.flush_pending {
            self.flush_pending = false;
            for lane in &mut self.lanes {
                lane.flush_held_into(out);
            }
        }
        let block = self.transport.advance(frames);
        if !block.running {
            return;
        }
        let frames_per_beat = self.sample_rate * 60.0 / block.tempo_bpm;
        for lane in &mut self.lanes {
            lane.render_block(block.start_beat, frames_per_beat, frames, self.fill, out);
        }
        // Lanes emitted in sequence; instances expect one timeline. The sort
        // is stable, so each lane's off-before-on ordering survives.
        out.sort_by_key(|event| event.frame);
    }

    /// What a surface shows. Cheap enough to poll.
    pub fn status(&self) -> SequencerStatusV1 {
        let snapshot = self.transport.snapshot();
        SequencerStatusV1 {
            running: snapshot.running,
            fill: self.fill,
            tempo_bpm: snapshot.tempo_bpm,
            beats_per_bar: snapshot.signature.beats_per_bar,
            beat_unit: snapshot.signature.beat_unit,
            bar: snapshot.bar,
            beat_in_bar: snapshot.beat_in_bar,
            beat_phase: snapshot.beat_phase,
            lanes: self
                .lanes
                .iter()
                .zip(&self.lane_names)
                .map(|(lane, name)| SequencerLaneStatus {
                    // A follow lane reads as playing only while its gate is
                    // open: the pad tells the truth about what sounds.
                    playing: lane.playing.is_some()
                        && (lane.follow.is_none() || !lane.input_keys.is_empty()),
                    queued: lane.pending.is_some(),
                    stopping: lane.stop_at.is_some(),
                    following: lane.follow.is_some(),
                    muted: lane.muted,
                    pattern_name: name.clone(),
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
                })
                .collect(),
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
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
            lane.render_block(beat, frames_per_beat, frames, false, &mut out);
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
            }],
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
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
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: TICKS_PER_BEAT / 4,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 38, // second pass of every two
                velocity: 100,
                channel: 0,
                probability: 100,
                condition: TrigCondition::Cycle { hit: 2, of: 2 },
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: TICKS_PER_BEAT / 2,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 42, // echoes the cycle note via pre
                velocity: 100,
                channel: 0,
                probability: 100,
                condition: TrigCondition::Pre,
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: 3 * TICKS_PER_BEAT / 4,
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 46, // fill only
                velocity: 100,
                channel: 0,
                probability: 100,
                condition: TrigCondition::Fill,
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
                lane.render_block(beat, frames_per_beat, 24_000, fill, &mut out);
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
        }];
        let roll_run = || {
            let mut lane = SequencerLane::new();
            lane.queue(CompiledPattern::compile(&chance).expect("valid"), 0.0);
            let mut fired = Vec::new();
            let mut beat = 0.0;
            for cycle in 0..32 {
                let mut out = Vec::new();
                lane.render_block(beat, 24_000.0, 24_000, false, &mut out);
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
        engine.render_block(16_384, &mut out);
        assert!(out.is_empty(), "a follow lane must be silent with no key held");
        assert!(engine.status().lanes[0].following);
        assert!(!engine.status().lanes[0].playing);

        // Hold F2: the phrase restarts on the next 16th, five semitones up.
        engine.note_input(53, true);
        out.clear();
        engine.render_block(16_384, &mut out);
        let ons: Vec<u8> = out
            .iter()
            .filter(|event| event.data[0] == 0x90)
            .map(|event| event.data[1])
            .collect();
        assert!(ons.iter().all(|&key| key == 53), "expected F2, got {ons:?}");
        assert!(!ons.is_empty());
        assert!(engine.status().lanes[0].playing);

        // Release: the ringing note is silenced at the next block.
        engine.note_input(53, false);
        out.clear();
        engine.render_block(16_384, &mut out);
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
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: TICKS_PER_BEAT / 4, // the off-sixteenth
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 38,
                velocity: 100,
                channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
            },
            rackforge_performance_api::PatternNoteSpec {
                tick: TICKS_PER_BEAT / 2, // the next pair's downbeat
                duration_ticks: TICKS_PER_BEAT / 4,
                key: 42,
                velocity: 100,
                channel: 0,
                    probability: 100,
                    condition: rackforge_performance_api::TrigCondition::Always,
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
            }],
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
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
            }],
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
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
                })
                .collect(),
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
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
                })
                .collect(),
            view: Default::default(),
            swing_percent: 50,
            root_key: 48,
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
        engine.render_block(4_096, &mut out);
        // At bar one beat zero the pattern begins on frame 0.
        assert_eq!(out[0].data, [0x90, 36, 100]);
        assert_eq!(out[0].frame, 0);
        assert!(engine.status().running);
        assert!(engine.status().lanes[0].playing);

        // Stop: silence is owed at the top of the next block, the pattern
        // stays armed.
        engine.apply(&SequencerCommand::TransportStop).expect("stop");
        out.clear();
        engine.render_block(4_096, &mut out);
        assert!(out.iter().all(|event| event.data[0] == 0x80));
        assert!(!out.is_empty(), "the sounding note was left ringing");
        assert!(engine.status().lanes[0].playing, "stop must not clear the lane");

        // Panic instead clears everything.
        engine.apply(&SequencerCommand::TransportPanic).expect("panic");
        out.clear();
        engine.render_block(4_096, &mut out);
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
        engine.render_block(16_384, &mut out);
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
            engine.render_block(16_384, &mut out);
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
            engine.render_block(16_384, &mut out);
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
        engine.render_block(16_384, &mut out);
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
        engine.render_block(16_384, &mut out);
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
