//! The host transport: one clock for the whole machine.
//!
//! RackForge is the owner of time for the same reason it is the owner of
//! threads. A sequencer, a metronome, an arpeggiating plugin and a hardware
//! controller's blinking LED must all agree on where beat one is, and they
//! only can if none of them keeps its own clock. This module is that clock,
//! and it deliberately lives outside every platform gate: the Linux LIVE host,
//! the Windows desktop, Android and the browser worklet all advance the same
//! arithmetic.
//!
//! # The contract
//!
//! **Samples are the single source of truth.** Musical position derives from
//! the sample position through a piecewise-linear map — an anchor sample, the
//! beat at that anchor, and the tempo since it. Changing the tempo re-anchors
//! the map at the current position instead of scaling history, so position
//! never jumps, accumulation never drifts, and the same sequence of calls
//! produces the same timeline on every platform, bit for bit.
//!
//! **The block is the unit of truth-telling.** [`Transport::advance`] moves
//! the clock by one audio block and reports every beat boundary inside it,
//! each with its exact frame offset. A consumer that quantises to those marks
//! is sample-accurate by construction and adds no latency: the launch, the
//! click or the pattern step lands inside the very block that crosses the
//! boundary.
//!
//! **Real-time discipline.** Nothing here allocates, blocks or syscalls.
//! Marks are returned in a bounded array sized for the worst legal case; the
//! bounds on tempo, signature and block size are what make that array a proof
//! rather than a hope.
//!
//! Stopping holds position — a paused show resumes where it was — and
//! [`Transport::locate`] is the explicit way to move the playhead.

/// Beats per minute the transport accepts. The bounds are musical, not
/// technical: below 20 a "beat" stops being a beat, above 400 the marks
/// array bound would need revisiting.
pub const MIN_TEMPO_BPM: f64 = 20.0;
pub const MAX_TEMPO_BPM: f64 = 400.0;

/// Frames one `advance` may cover. Every RackForge host renders in blocks
/// well under this; the bound exists so `MAX_MARKS_PER_BLOCK` is provable.
pub const MAX_BLOCK_FRAMES: u32 = 16_384;

/// Sample rates the transport accepts, matching what the audio hosts open.
pub const MIN_SAMPLE_RATE: f64 = 8_000.0;
pub const MAX_SAMPLE_RATE: f64 = 384_000.0;

/// Worst case: 400 bpm at 8 kHz gives 1200 frames per beat, so a 16384-frame
/// block crosses at most ceil(16384 / 1200) + 1 = 15 beats.
pub const MAX_MARKS_PER_BLOCK: usize = 16;

/// A time signature. `beats_per_bar` counts transport beats; the tempo counts
/// those same beats per minute. The note value of the denominator is kept for
/// display and for consumers that phrase in it — the clock itself only needs
/// to know how many beats make a bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeSignature {
    pub beats_per_bar: u8,
    pub beat_unit: u8,
}

impl TimeSignature {
    pub const fn new(beats_per_bar: u8, beat_unit: u8) -> Option<Self> {
        if beats_per_bar < 1 || beats_per_bar > 32 {
            return None;
        }
        if !matches!(beat_unit, 1 | 2 | 4 | 8 | 16 | 32) {
            return None;
        }
        Some(Self {
            beats_per_bar,
            beat_unit,
        })
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            beats_per_bar: 4,
            beat_unit: 4,
        }
    }
}

/// One beat boundary inside an advanced block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatMark {
    /// Frame offset inside the block, `0..frames`.
    pub frame: u32,
    /// The absolute beat index this mark begins, counted from beat zero.
    pub beat: u64,
    /// Whether this beat is also the start of a bar.
    pub is_bar: bool,
}

/// What one [`Transport::advance`] call reports.
#[derive(Clone, Copy, Debug)]
pub struct TransportBlock {
    /// Whether the clock moved. A stopped transport reports no marks.
    pub running: bool,
    /// Musical position at the first frame of the block, in beats.
    pub start_beat: f64,
    /// The tempo the whole block was advanced at.
    pub tempo_bpm: f64,
    /// Beat boundaries inside the block, in ascending frame order.
    marks: [BeatMark; MAX_MARKS_PER_BLOCK],
    mark_count: usize,
}

impl TransportBlock {
    pub fn marks(&self) -> &[BeatMark] {
        &self.marks[..self.mark_count]
    }
}

/// A snapshot for surfaces: what a display shows, not what a sequencer runs
/// on. Bars and beats are 1-based here because that is how musicians count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransportSnapshot {
    pub running: bool,
    pub tempo_bpm: f64,
    pub signature: TimeSignature,
    pub bar: u64,
    pub beat_in_bar: u8,
    /// Progress through the current beat, `0.0..1.0`.
    pub beat_phase: f64,
}

/// The clock. One per host, owned by the session, advanced by the audio
/// callback and read by everything else through snapshots.
#[derive(Clone, Debug)]
pub struct Transport {
    sample_rate: f64,
    tempo_bpm: f64,
    signature: TimeSignature,
    running: bool,
    /// Absolute sample position; the single source of truth.
    position_samples: u64,
    /// The piecewise-linear map: at `anchor_samples` the position was
    /// `anchor_beat`, and the current tempo has applied ever since.
    anchor_samples: u64,
    anchor_beat: f64,
}

impl Transport {
    /// A stopped transport at bar one, beat one.
    pub fn new(sample_rate: f64, tempo_bpm: f64) -> Option<Self> {
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return None;
        }
        if !(MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&tempo_bpm) {
            return None;
        }
        Some(Self {
            sample_rate,
            tempo_bpm,
            signature: TimeSignature::default(),
            running: false,
            position_samples: 0,
            anchor_samples: 0,
            anchor_beat: 0.0,
        })
    }

    pub fn set_signature(&mut self, signature: TimeSignature) {
        self.signature = signature;
    }

    pub const fn signature(&self) -> TimeSignature {
        self.signature
    }

    pub const fn is_running(&self) -> bool {
        self.running
    }

    pub const fn tempo_bpm(&self) -> f64 {
        self.tempo_bpm
    }

    fn frames_per_beat(&self) -> f64 {
        self.sample_rate * 60.0 / self.tempo_bpm
    }

    /// Musical position at an absolute sample, in beats.
    fn beat_at(&self, samples: u64) -> f64 {
        self.anchor_beat + (samples - self.anchor_samples) as f64 / self.frames_per_beat()
    }

    /// Musical position now, in beats.
    pub fn position_beats(&self) -> f64 {
        self.beat_at(self.position_samples)
    }

    /// Starts the clock from wherever it stands. Starting is immediate:
    /// quantised *launches* are a consumer decision made against the marks,
    /// not a transport state.
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Stops the clock, holding position. A paused show resumes where it was.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Moves the playhead to an absolute beat. The only way position jumps.
    pub fn locate(&mut self, beat: f64) {
        let beat = beat.max(0.0);
        self.anchor_samples = self.position_samples;
        self.anchor_beat = beat;
    }

    /// Changes tempo by re-anchoring at the current position, so the timeline
    /// bends forward from here rather than rewriting where beats already
    /// fell. Out-of-range tempos are clamped, never rejected: a tap slightly
    /// out of bounds mid-show must not leave the old tempo standing.
    pub fn set_tempo(&mut self, tempo_bpm: f64) {
        let clamped = tempo_bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM);
        self.anchor_beat = self.position_beats();
        self.anchor_samples = self.position_samples;
        self.tempo_bpm = clamped;
    }

    /// The next beat boundary at or after the current position — where a
    /// quantised launch lands.
    pub fn next_beat(&self) -> u64 {
        let position = self.position_beats();
        let ceil = position.ceil();
        // On the boundary means this beat, not the next one.
        if (position - position.floor()).abs() < f64::EPSILON {
            position as u64
        } else {
            ceil as u64
        }
    }

    /// The first beat of the next bar at or after the current position.
    pub fn next_bar(&self) -> u64 {
        let beats = self.signature.beats_per_bar as u64;
        let next = self.next_beat();
        next.div_ceil(beats) * beats
    }

    /// Advances the clock by one audio block and reports the beat boundaries
    /// inside it. Stopped, it reports the standing position and no marks.
    ///
    /// `frames` must not exceed [`MAX_BLOCK_FRAMES`]; that bound is what makes
    /// the marks array provable. An oversized block is clamped — and loudly
    /// asserted in debug builds — rather than silently reinterpreted, because
    /// a clock that quietly loses time is worse than one that refuses it.
    pub fn advance(&mut self, frames: u32) -> TransportBlock {
        debug_assert!(
            frames <= MAX_BLOCK_FRAMES,
            "audio blocks larger than MAX_BLOCK_FRAMES are outside the contract"
        );
        let frames = frames.min(MAX_BLOCK_FRAMES);
        let start_beat = self.position_beats();
        let mut block = TransportBlock {
            running: self.running,
            start_beat,
            tempo_bpm: self.tempo_bpm,
            marks: [BeatMark {
                frame: 0,
                beat: 0,
                is_bar: false,
            }; MAX_MARKS_PER_BLOCK],
            mark_count: 0,
        };
        if !self.running || frames == 0 {
            return block;
        }

        let frames_per_beat = self.frames_per_beat();
        let beats_per_bar = self.signature.beats_per_bar as u64;
        // The first whole beat at or after the block start. A mark exactly on
        // frame zero belongs to this block: the boundary is the first sample
        // of the beat.
        let mut beat = start_beat.ceil() as u64;
        if (start_beat - start_beat.floor()).abs() < f64::EPSILON {
            beat = start_beat as u64;
        }
        loop {
            // Each mark is computed from the anchor, not accumulated, so a
            // long run keeps beats exactly where the map says they are.
            let beat_samples =
                self.anchor_samples as f64 + (beat as f64 - self.anchor_beat) * frames_per_beat;
            let offset = beat_samples - self.position_samples as f64;
            if offset >= frames as f64 {
                break;
            }
            if block.mark_count == MAX_MARKS_PER_BLOCK {
                debug_assert!(false, "beat marks exceeded the provable bound");
                break;
            }
            block.marks[block.mark_count] = BeatMark {
                frame: offset.max(0.0) as u32,
                beat,
                is_bar: beat.is_multiple_of(beats_per_bar),
            };
            block.mark_count += 1;
            beat += 1;
        }

        self.position_samples += frames as u64;
        block
    }

    /// What a display shows.
    pub fn snapshot(&self) -> TransportSnapshot {
        let position = self.position_beats();
        let beats_per_bar = self.signature.beats_per_bar as u64;
        let whole = position.floor().max(0.0) as u64;
        TransportSnapshot {
            running: self.running,
            tempo_bpm: self.tempo_bpm,
            signature: self.signature,
            bar: whole / beats_per_bar + 1,
            beat_in_bar: (whole % beats_per_bar) as u8 + 1,
            beat_phase: position - position.floor(),
        }
    }
}

/// Tempo from tap intervals, the way a player taps it on stage.
///
/// Takes the timestamps of the taps in seconds, newest last. Uses up to the
/// last five taps, drops the session when a gap says the player started over
/// (more than two seconds, or twice the running interval), and needs two taps
/// to say anything. Returns beats per minute within the transport's bounds.
pub fn tap_tempo(taps_seconds: &[f64]) -> Option<f64> {
    if taps_seconds.len() < 2 {
        return None;
    }
    let recent = &taps_seconds[taps_seconds.len().saturating_sub(5)..];
    let mut intervals = [0.0_f64; 4];
    let mut count = 0;
    for pair in recent.windows(2) {
        let interval = pair[1] - pair[0];
        if interval <= 0.0 {
            return None;
        }
        intervals[count] = interval;
        count += 1;
    }
    // A long gap or a sudden doubling means a new tapping session: keep only
    // the intervals after the break.
    let mut start = 0;
    for index in 1..count {
        let previous = intervals[index - 1];
        let current = intervals[index];
        if current > 2.0 || current > previous * 2.0 || current < previous / 2.0 {
            start = index;
        }
    }
    let used = &intervals[start..count];
    if used.is_empty() {
        return None;
    }
    let mean = used.iter().sum::<f64>() / used.len() as f64;
    let bpm = 60.0 / mean;
    Some(bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> Transport {
        Transport::new(48_000.0, 120.0).expect("valid transport")
    }

    /// Advances by any span using legal block sizes, collecting every mark.
    /// Tests think in beats-worth of frames; real hosts never exceed the
    /// block bound, so the helper is the tests' bridge to the contract.
    fn run(clock: &mut Transport, mut frames: u32) -> Vec<BeatMark> {
        let mut marks = Vec::new();
        while frames > 0 {
            let step = frames.min(4_096);
            marks.extend_from_slice(clock.advance(step).marks());
            frames -= step;
        }
        marks
    }

    #[test]
    fn bounds_are_enforced_at_creation() {
        assert!(Transport::new(48_000.0, 10.0).is_none());
        assert!(Transport::new(48_000.0, 500.0).is_none());
        assert!(Transport::new(1_000.0, 120.0).is_none());
        assert!(Transport::new(48_000.0, 120.0).is_some());
    }

    #[test]
    fn a_stopped_transport_reports_no_marks_and_holds_position() {
        let mut clock = transport();
        let block = clock.advance(512);
        assert!(!block.running);
        assert!(block.marks().is_empty());
        assert_eq!(clock.position_beats(), 0.0);
    }

    #[test]
    fn beat_marks_fall_on_exact_frames() {
        let mut clock = transport();
        clock.start();
        // 120 bpm at 48 kHz: a beat every 24 000 frames. Two beats' worth of
        // audio carries exactly beats 0 and 1, each on its first sample.
        let marks = run(&mut clock, 48_000);
        assert_eq!(
            marks,
            vec![
                BeatMark {
                    frame: 0,
                    beat: 0,
                    is_bar: true
                },
                // 24 000 = 5 * 4 096 + 3 520: the mark lands mid-block.
                BeatMark {
                    frame: 3_520,
                    beat: 1,
                    is_bar: false
                },
            ]
        );
    }

    #[test]
    fn beats_inside_a_block_carry_their_offsets() {
        let mut clock = transport();
        clock.start();
        run(&mut clock, 23_000);
        // The next block crosses beat one at offset 1 000.
        let block = clock.advance(4_096);
        assert_eq!(block.marks().len(), 1);
        assert_eq!(
            block.marks()[0],
            BeatMark {
                frame: 1_000,
                beat: 1,
                is_bar: false
            }
        );
    }

    #[test]
    fn bars_follow_the_signature() {
        let mut clock = transport();
        clock.set_signature(TimeSignature::new(3, 4).expect("valid"));
        clock.start();
        let marks = run(&mut clock, 24_000 * 10);
        let bars: Vec<u64> = marks
            .iter()
            .filter(|mark| mark.is_bar)
            .map(|mark| mark.beat)
            .collect();
        assert_eq!(bars, vec![0, 3, 6, 9]);
    }

    #[test]
    fn tempo_change_reanchors_without_moving_the_playhead() {
        let mut clock = transport();
        clock.start();
        run(&mut clock, 36_000); // a beat and a half in
        let before = clock.position_beats();
        clock.set_tempo(240.0);
        let after = clock.position_beats();
        assert!(
            (before - after).abs() < 1e-9,
            "position moved on tempo change"
        );
        // At 240 bpm a beat is 12 000 frames: beat 2 lands 6 000 frames on.
        let marks = run(&mut clock, 12_000);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].frame, 1_904); // 6 000 = 4 096 + 1 904
        assert_eq!(marks[0].beat, 2);
    }

    #[test]
    fn out_of_range_tempo_clamps_instead_of_standing() {
        let mut clock = transport();
        clock.set_tempo(1_000.0);
        assert_eq!(clock.tempo_bpm(), MAX_TEMPO_BPM);
        clock.set_tempo(1.0);
        assert_eq!(clock.tempo_bpm(), MIN_TEMPO_BPM);
    }

    #[test]
    fn advance_is_deterministic_across_identical_runs() {
        let run = || {
            let mut clock = transport();
            clock.set_signature(TimeSignature::new(7, 8).expect("valid"));
            clock.start();
            let mut all = Vec::new();
            for step in 0..500 {
                if step == 200 {
                    clock.set_tempo(173.0);
                }
                for mark in clock.advance(481).marks() {
                    all.push((mark.frame, mark.beat, mark.is_bar));
                }
            }
            all
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn marks_never_exceed_the_provable_bound() {
        let mut clock = Transport::new(MIN_SAMPLE_RATE, MAX_TEMPO_BPM).expect("valid");
        clock.start();
        for _ in 0..64 {
            let block = clock.advance(MAX_BLOCK_FRAMES);
            assert!(block.marks().len() <= MAX_MARKS_PER_BLOCK);
            // Every mark ascends and stays inside the block.
            for pair in block.marks().windows(2) {
                assert!(pair[0].frame <= pair[1].frame);
                assert_eq!(pair[0].beat + 1, pair[1].beat);
            }
        }
    }

    #[test]
    fn quantised_launch_points_land_on_the_grid() {
        let mut clock = transport();
        clock.start();
        run(&mut clock, 30_000); // 1.25 beats in
        assert_eq!(clock.next_beat(), 2);
        assert_eq!(clock.next_bar(), 4);
        // Standing exactly on a bar keeps that bar as the launch point.
        clock.locate(8.0);
        assert_eq!(clock.next_beat(), 8);
        assert_eq!(clock.next_bar(), 8);
    }

    #[test]
    fn stop_holds_and_locate_moves() {
        let mut clock = transport();
        clock.start();
        run(&mut clock, 48_000);
        clock.stop();
        run(&mut clock, 48_000);
        assert!((clock.position_beats() - 2.0).abs() < 1e-9);
        clock.locate(0.0);
        assert_eq!(clock.position_beats(), 0.0);
    }

    #[test]
    fn snapshot_counts_the_way_musicians_do() {
        let mut clock = transport();
        clock.start();
        run(&mut clock, 24_000 * 5 + 12_000); // beat 5.5: bar 2, beat 2, phase .5
        let snapshot = clock.snapshot();
        assert_eq!(snapshot.bar, 2);
        assert_eq!(snapshot.beat_in_bar, 2);
        assert!((snapshot.beat_phase - 0.5).abs() < 1e-9);
    }

    #[test]
    fn tap_tempo_reads_steady_taps() {
        let taps: Vec<f64> = (0..4).map(|tap| tap as f64 * 0.5).collect();
        let bpm = tap_tempo(&taps).expect("tempo from steady taps");
        assert!((bpm - 120.0).abs() < 1e-9);
    }

    #[test]
    fn tap_tempo_drops_a_stale_session() {
        // Two old taps, a long pause, then fresh taps at 100 bpm.
        let taps = [0.0, 0.6, 10.0, 10.6, 11.2];
        let bpm = tap_tempo(&taps).expect("tempo from the fresh session");
        assert!((bpm - 100.0).abs() < 1e-6);
    }

    #[test]
    fn tap_tempo_needs_two_taps_and_monotonic_time() {
        assert!(tap_tempo(&[1.0]).is_none());
        assert!(tap_tempo(&[2.0, 1.0]).is_none());
    }
}
