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

use crate::transport::{TimeSignature, Transport};
use rackforge_control_api::{
    SequencerCommand, SequencerLaneStatus, SequencerQuantize, SequencerStatusV1,
};
use rackforge_performance_api::{
    MAX_PATTERN_NOTES, MAX_PATTERN_TICKS, PATTERN_TICKS_PER_BEAT, PatternDefinition,
};
use rackforge_plugin_api::abi::MidiEventV1;

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
    channel: u8,
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
            let start_beat = note.tick as f64 / TICKS_PER_BEAT as f64;
            let end_tick = note.tick.saturating_add(note.duration_ticks).min(document.length_ticks);
            notes.push(CompiledNote {
                start_beat,
                duration_beats: (end_tick - note.tick) as f64 / TICKS_PER_BEAT as f64,
                key: note.key,
                velocity: note.velocity,
                channel: note.channel,
            });
        }
        notes.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
        Ok(Self {
            length_beats: document.length_ticks as f64 / TICKS_PER_BEAT as f64,
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

/// A pattern armed to begin at an absolute beat.
struct PendingLaunch {
    pattern: CompiledPattern,
    at_beat: f64,
}

/// A pattern currently sounding, anchored at the beat it began.
struct PlayingPattern {
    pattern: CompiledPattern,
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
    playing: Option<PlayingPattern>,
    pending: Option<PendingLaunch>,
    /// The beat past which no new notes start, when a stop is queued.
    stop_at: Option<f64>,
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
        Self {
            playing: None,
            pending: None,
            stop_at: None,
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
        self.stop_at = None;
        self.pending = Some(PendingLaunch { pattern, at_beat });
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
        if let Some(playing) = &self.playing {
            emit_pattern_notes(
                playing,
                self.muted,
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
    muted: bool,
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
            if muted {
                continue;
            }
            // Off headroom is what guarantees a saturated block thins the
            // music instead of sustaining it forever.
            if staged.len() >= MAX_EVENTS_PER_BLOCK - OFF_HEADROOM || held.len() >= MAX_HELD_NOTES {
                *dropped_notes += 1;
                continue;
            }
            staged.push(StagedEvent {
                frame: frame_of(at, block_start, frames_per_beat, frames),
                on: true,
                data: note_on(note.key, note.velocity, note.channel),
            });
            held.push(HeldNote {
                key: note.key,
                channel: note.channel,
                off_beat: at + note.duration_beats,
            });
        }
    }
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
/// control on the surface.
pub const MAX_SEQUENCER_LANES: usize = 8;

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
}

impl SequencerEngine {
    /// A stopped engine at bar one, 120 bpm, four-four.
    pub fn new(sample_rate: f64) -> Option<Self> {
        let transport = Transport::new(sample_rate, 120.0)?;
        Some(Self {
            sample_rate,
            transport,
            lanes: (0..MAX_SEQUENCER_LANES).map(|_| SequencerLane::new()).collect(),
            lane_names: vec![None; MAX_SEQUENCER_LANES],
            flush_pending: false,
            panic_pending: false,
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
            lane.render_block(block.start_beat, frames_per_beat, frames, out);
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
                    playing: lane.playing.is_some(),
                    queued: lane.pending.is_some(),
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
                })
                .collect(),
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
            lane.render_block(beat, frames_per_beat, frames, &mut out);
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
            }],
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
            }],
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
            }],
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
                })
                .collect(),
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
                })
                .collect(),
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
