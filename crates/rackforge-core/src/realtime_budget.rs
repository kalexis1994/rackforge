//! What a plugin may spend on one real-time call, in the plugin's own units.
//!
//! A physically modelled instrument costs what its model costs, and the
//! machines RackForge runs on differ by nearly an order of magnitude. Until
//! this existed, a plugin was told how many frames to render and never how
//! much machine there was to render them with, so every plugin was calibrated
//! against whatever desk its author sat at. Concert Grand carries the
//! evidence: `PARTIAL_BUDGET` is documented in its own source as "a fuel
//! budget, not a taste one", sized against a 512-frame block on a desktop --
//! and a Raspberry Pi gets the same number and breaks up.
//!
//! So the host measures what it alone can measure, and hands it over.
//!
//! # Why the host decides on time and pays in fuel
//!
//! Two different quantities, for two different jobs.
//!
//! **The decision is made on wall time**, because a deadline is wall time: a
//! block that took too long is an xrun whatever it cost in any other unit.
//! The governor watches how often this slot's render ran past its share of the
//! period, which is the thing that is actually wrong when something is wrong.
//!
//! **The budget is denominated in fuel**, because a plugin cannot read a
//! clock. Fuel is wasmtime's instruction counter: the same block of audio
//! costs the same fuel on a phone, a Pi and a desktop, because it is the same
//! work. A number in fuel therefore means something inside the plugin, and the
//! same budget produces the same choice on every machine of the same speed --
//! which is what makes it testable, since a test injects a rate instead of
//! racing a timer.
//!
//! # Why the loop is closed, and why that matters more than the arithmetic
//!
//! The obvious design hands the plugin a budget computed from a cost model and
//! trusts it to fit. That was tried and the model would not hold still:
//! Concert Grand's own cost per voice, measured through this crate's `stress`
//! command, came out about three times higher for freshly struck notes than
//! for notes a Raspberry Pi had been holding for twelve seconds, because the
//! partial cull had thinned them in between. Any constant baked into the
//! plugin would have been wrong for one of those two, and quietly.
//!
//! So nothing here trusts a cost model. The governor publishes a budget,
//! watches what the slot then actually does with the period, and multiplies
//! the budget up or down until the render fits. A plugin whose arithmetic is
//! off by three still converges; it just takes a few seconds longer. The first
//! budget is a feed-forward guess from the measured rate, and everything after
//! it is feedback.
//!
//! # Why it is slow, one-directional and sticky
//!
//! Quality that oscillates with CPU noise sounds worse than lower quality held
//! steady -- the timbre would breathe with the load. So the governor falls
//! quickly when blocks are running past their allowance, and rises only after
//! the machine has been comfortable for a long time. It never publishes a
//! change that is not material, because a plugin is allowed to rebuild
//! coefficients when it is told.
//!
//! # Plugins that do not participate
//!
//! Most will not, and that is their author's choice. A plugin with no budget
//! export is never called and never knows this happened; the fuel cap in
//! `RuntimeLimits` still stops a runaway, and per-slot telemetry still names
//! which plugin is over. Nothing here is required to render audio.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Share of the block deadline one slot may plan to spend.
///
/// Not 1.0, and not close to it. The deadline is when the buffer is due, not
/// when it is comfortable: the engine also has to convert and write the
/// period, ALSA has to hand it over, and a governor may drop the clock in the
/// middle of the block being planned.
///
/// 0.6 is measured rather than chosen. On a Raspberry Pi 4 at 128 frames,
/// Concert Grand holding six notes with a 106-mode soundboard averaged 1601 us
/// of the 2667 us period -- 60 % -- and missed no deadlines at all across the
/// window; the same six notes at 1892 us (71 %) missed seventeen.
pub const DEFAULT_HEADROOM: f64 = 0.6;

/// Blocks to watch before the rate is worth believing. At 128 frames this is
/// under a second, and it is long enough to leave the first blocks after a
/// device change -- which are always the slowest -- out of the estimate.
const MINIMUM_OBSERVATIONS: u32 = 96;

/// Weight of one block in the estimates. Slow on purpose: they should describe
/// the machine, not the phrase being played.
const SMOOTHING: f64 = 1.0 / 256.0;

/// How often a change may be published at all, and the window over which
/// blocks that ran long are counted.
const PUBLISH_INTERVAL: Duration = Duration::from_secs(2);

/// How long the slot must be comfortable before quality is given back.
const RAISE_AFTER: Duration = Duration::from_secs(20);

/// How long the instrument must have gone without a note before quality is
/// given back. Not a performance detail: a raise rebuilds banks, and a player
/// hears a soundboard change shape under their hands. Measured on the
/// appliance and reported by the player as "se nota como cambia la calidad
/// en vivo" -- the raise was landing between phrases of a piece, twenty
/// seconds after the dense passage that had cut it. Cuts still land whenever
/// blocks run late, because the alternative to a cut is an xrun; raises wait
/// for the instrument to be silent, where nothing can be heard changing.
pub const SILENT_BEFORE_RAISE: Duration = Duration::from_secs(30);

/// Blocks allowed to run late before the budget is cut, as a fraction of the
/// window. Not zero: a late block in a hundred is a scheduler hiccup, and
/// chasing it would mean a permanently thinner instrument. Measured on the
/// appliance at one percent with the line below at 0.9: twelve held notes
/// missed no deadline at all, and the governor still cut five times on the
/// tail -- each one a rebuild the player could hear as the piano thinning
/// under their hands, for nothing.
const OVER_TOLERANCE: f64 = 0.02;

/// How many windows in a row have to be over the tolerance before the budget
/// is cut.
///
/// One was the original answer, on the grounds that a late block is audible
/// now and a cut is the alternative to an xrun. Measured on the appliance,
/// that is true of the block and false of the cut.
///
/// A chord is a burst. Twelve notes struck together put about half a second
/// of late blocks into one two-second window and nothing into the next: the
/// same twelve notes HELD cost 51 % of the period and miss nothing at all.
/// Cutting on that window thins the instrument permanently for a transient
/// that has already passed -- and, measured, the cut does not even reduce
/// the burst. A ramp run with the plugin declining the budget entirely, so
/// no cut ever happened, missed 271 deadlines at twelve notes; the same ramp
/// with the governor cutting twice, down to 1.78 M fuel from 2.90 M, missed
/// 225 and 251. A 1.6x thinner instrument had the same transient.
///
/// So the governor now asks for the lateness to still be there two windows
/// later. A burst is gone by then; an instrument that genuinely does not fit
/// is not. The cost of the change is stated plainly: a real overload is cut
/// two seconds later than it used to be, which is two more seconds of xruns
/// before the mechanism that stops them engages.
const CONFIRM_WINDOWS: u32 = 2;

/// How much of the period a block has to reach to count as late.
///
/// Deliberately not the allowance. The allowance is what the budget is sized
/// against -- a target with room left over -- and treating a block that
/// exceeded it as a failure makes the governor act on renders that are
/// perfectly fine: measured on a Raspberry Pi, two notes rendered in 63 % of
/// the period and missed nothing at all, while sitting above a 60 % allowance
/// and being read as trouble. What is actually wrong is a block that is about
/// to arrive late, so that is what this measures.
///
/// One, and the reason is measured. At 0.9 and then 0.95 the governor was
/// still cutting through ramps that missed **no deadline at all** -- twelve
/// held notes on a Raspberry Pi, p99 of 2228 us against a 2666 us period,
/// zero misses, three cuts. What crosses a line drawn inside the period is
/// an ordinary expensive block: a note-on runs the strike simulation, and
/// that block costs more than its neighbours without being late for
/// anything. Cutting the instrument for it is thinning a piano that was
/// keeping up.
///
/// A slot's share of the whole period is the honest line: past it the slot
/// alone has spent the buffer, and something is going to arrive late.
const LATE_AT: f64 = 1.0;

/// The slot has to be using less than this much of its allowance before any
/// of it is given back. The gap between this and 1.0 is the hysteresis that
/// keeps the timbre from breathing with the load.
const COMFORTABLE: f64 = 0.7;

/// Bounds on one correction. The floor stops a single bad window from
/// collapsing the instrument; the ceiling makes sure a cut is a real cut even
/// when the mean looks fine and only the tail is late.
const MOST_SEVERE_CUT: f64 = 0.5;
/// Below `1.0 - MATERIAL_CHANGE`, so that every cut the governor decides to
/// make is one it can actually publish. A cut rejected for being too small to
/// bother the plugin with still looks like a cut that did not work.
const GENTLEST_CUT: f64 = 0.8;

/// Consecutive cuts that bought nothing before the governor accepts that this
/// plugin is not going to fit and stops cutting.
///
/// A budget that keeps falling while the render does not is not controlling
/// anything -- it is thinning an instrument for no reason, and on the way down
/// it reads every cut as evidence that another is needed. Measured on a
/// Raspberry Pi before this existed: eight cuts in sixteen seconds took the
/// budget from 340,068 fuel to 5,649 while the render went the wrong way,
/// from 1,050 us to 3,598 us.
const STUBBORN_LIMIT: u32 = 3;

/// A cut has to buy at least this much of the load back to count as working.
const CUT_MUST_BUY: f64 = 0.95;

/// How much is given back at a time, once it has been earned.
const RAISE_BY: f64 = 1.25;

/// A change smaller than this is not worth a rebuild on the plugin's side.
const MATERIAL_CHANGE: f64 = 0.15;

/// How long a budget has to stand still, under observation, before it is
/// the number this machine gets remembered by. `exhausted` counts at once.
const SETTLE_AFTER: Duration = Duration::from_secs(60);

/// A budget is never published below this. A plugin handed a budget it cannot
/// render anything with should be told nothing instead, and left at whatever
/// its author shipped: an instrument that is silent is worse than one that is
/// late, and at that point the honest report is that this machine cannot run
/// this plugin at all.
const MINIMUM_CREDIBLE_BUDGET: u64 = 4_096;

/// Why a budget changed, for the log line and for the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetReason {
    /// First budget this plugin has been given on this machine.
    Measured,
    /// Blocks were running past their allowance.
    Tightened,
    /// The slot has been comfortable long enough to afford more.
    Relaxed,
    /// Cutting stopped helping. The budget stays where it is and the blocks
    /// keep running long: this machine cannot render this plugin at this
    /// period, and saying so is more use than thinning it further.
    Exhausted,
    /// The budget this machine settled on last time, handed over on the
    /// first block so the instrument is built right before anyone plays it.
    Seeded,
    /// Not a budget change: the note, for the log and the store, that the
    /// budget has stopped moving and is worth remembering.
    Settled,
}

impl BudgetReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::Tightened => "tightened",
            Self::Relaxed => "relaxed",
            Self::Exhausted => "exhausted",
            Self::Seeded => "seeded",
            Self::Settled => "settled",
        }
    }
}

/// One slot's view of how fast this machine is, and what it may spend.
///
/// Fed one observation per rendered block, and asked -- at the block rate,
/// cheaply -- whether it has something new to say.
#[derive(Debug)]
pub struct BudgetGovernor {
    deadline_ns: u64,
    share: f64,
    headroom: f64,
    /// This machine's speed for this plugin: the only machine-dependent
    /// quantity in the system, and the one a plugin cannot measure itself.
    ns_per_fuel: f64,
    render_ns: f64,
    observations: u32,
    /// Blocks in the current window, and how many of them ran past the
    /// allowance. A rate, not a total, so the window length does not matter.
    window_blocks: u32,
    window_over: u32,
    published: Option<u64>,
    last_publish: Option<Duration>,
    /// When the window was last read, which is not the same as when
    /// something was last published.
    ///
    /// They used to be one field, and the cadence hung off it: a poll that
    /// got past the interval, read the window and then decided the change
    /// was too small to publish left the timer stale, so every block after
    /// it passed the interval too and was judged **on its own**. Measured on
    /// the appliance with the counts in the log: `blocks=1 late=1`, a
    /// hundred percent of a window of one. Every cut this mechanism has ever
    /// made on a ramp that missed no deadline was a single unlucky block --
    /// a note-on running the strike simulation, or a bank rebuild -- read as
    /// the whole instrument being late.
    last_window_at: Option<Duration>,
    comfortable_since: Option<Duration>,
    /// The load the last cut was made at, and how many cuts in a row have
    /// failed to bring it down.
    load_at_cut: Option<f64>,
    stubborn: u32,
    exhausted: bool,
    /// What was published before the current streak of cuts began. If the
    /// streak ends in `Exhausted` the cuts bought nothing, so this -- and not
    /// the floor they reached -- is what this machine actually runs at.
    before_streak: Option<u64>,
    last_over_rate: f64,
    last_load: f64,
    last_blocks: u32,
    last_over: u32,
    /// A remembered budget waiting for the first poll to hand it over.
    seed_pending: bool,
    /// Consecutive windows over `OVER_TOLERANCE`. A cut waits for
    /// `CONFIRM_WINDOWS` of them, so that a chord's burst -- which fills one
    /// window and leaves the next clean -- does not thin the instrument for
    /// a transient it cannot fix.
    late_streak: u32,
    /// The window after a publish is the plugin rebuilding to it, and a
    /// rebuild is heavy: measured on the appliance, every cut's own rebuild
    /// ran late, that lateness justified the next cut, and twelve held notes
    /// that missed no deadline were cut five times in a row. So the first
    /// window after any publish is thrown away unread.
    discard_next_window: bool,
}

impl Default for BudgetGovernor {
    /// A governor that has been told nothing yet, and so says nothing. A voice
    /// is built before the audio loop knows its period, and a governor that
    /// guessed a deadline in the meantime would hand out a budget for a
    /// machine it has not measured.
    fn default() -> Self {
        Self {
            deadline_ns: 0,
            share: 1.0,
            headroom: DEFAULT_HEADROOM,
            ns_per_fuel: 0.0,
            render_ns: 0.0,
            observations: 0,
            window_blocks: 0,
            window_over: 0,
            published: None,
            last_publish: None,
            last_window_at: None,
            comfortable_since: None,
            load_at_cut: None,
            stubborn: 0,
            exhausted: false,
            before_streak: None,
            last_over_rate: 0.0,
            last_load: 0.0,
            last_blocks: 0,
            last_over: 0,
            seed_pending: false,
            late_streak: 0,
            discard_next_window: false,
        }
    }
}

impl BudgetGovernor {
    /// The situation this slot renders in, restated every block by the audio
    /// loop because both halves of it can change under a running engine: the
    /// period when the interface is re-bound, the slot count when the rack is
    /// edited.
    ///
    /// Slots are given equal shares of the headroom rather than shares
    /// proportional to what they have been spending: a slot that spends more
    /// would otherwise be granted more for spending it, and the loudest plugin
    /// in the rack would squeeze out the rest.
    ///
    /// Cheap and idempotent -- two stores, no state reset.
    pub fn configure(&mut self, deadline_ns: u64, slot_count: usize) {
        self.deadline_ns = deadline_ns;
        self.share = 1.0 / slot_count.max(1) as f64;
    }

    /// The budget currently handed to the plugin, if any.
    pub const fn published(&self) -> Option<u64> {
        self.published
    }

    /// The period this governor was last configured for.
    pub const fn deadline_ns(&self) -> u64 {
        self.deadline_ns
    }

    /// Starts from a budget this machine settled on before. It is handed to
    /// the plugin on the very next poll, without waiting for a rate to be
    /// measured: what the last session learned under load is a better first
    /// guess than anything the first second of this one can say, and it lands
    /// before the first note instead of in the middle of a phrase.
    pub fn seed(&mut self, budget: u64) {
        if budget < MINIMUM_CREDIBLE_BUDGET {
            return;
        }
        self.published = Some(budget);
        self.seed_pending = true;
        self.exhausted = false;
        self.stubborn = 0;
        self.load_at_cut = None;
    }

    /// The budget worth writing down for next time, once it has stopped
    /// moving: the published one when the governor has given up on cutting
    /// further, or when nothing has changed for `SETTLE_AFTER`.
    pub fn settled(&self, now: Duration) -> Option<u64> {
        let published = self.published?;
        let last = self.last_publish?;
        if self.exhausted {
            // The streak that ended here bought nothing -- that is what
            // `Exhausted` means -- so the floor it reached is not a fact about
            // this machine, it is the record of a mistake. Remember what was
            // running before it started.
            //
            // Without this the store ratchets: a session inherits the floor,
            // cuts from there, writes a lower floor, and the instrument gets
            // smaller every time the engine starts. Measured on a Raspberry
            // Pi across three sessions: 4,677,176 fuel, then 582,949, then
            // 373,087 -- the last of which is the plugin's own minimum
            // quality, reached without the machine ever missing a deadline.
            return self.before_streak.or(Some(published));
        }
        (now.saturating_sub(last) >= SETTLE_AFTER).then_some(published)
    }

    /// This machine's measured speed for this plugin, once it is known.
    pub fn nanoseconds_per_fuel(&self) -> Option<f64> {
        (self.observations >= MINIMUM_OBSERVATIONS && self.ns_per_fuel > 0.0)
            .then_some(self.ns_per_fuel)
    }

    /// The wall time this slot is aiming to spend on one block: what its
    /// budget is sized against.
    fn allowance_ns(&self) -> f64 {
        self.deadline_ns as f64 * self.headroom * self.share
    }

    /// The wall time past which this slot's block counts as late: what the
    /// governor actually acts on.
    fn late_ns(&self) -> f64 {
        self.deadline_ns as f64 * LATE_AT * self.share
    }

    /// One rendered block: how long it took, and what the plugin spent.
    ///
    /// A block with no fuel reading is a plugin the sandbox does not meter --
    /// a native plugin, or the browser runtime. Those never accumulate a rate,
    /// so they are never handed a budget, which is the correct outcome: a
    /// budget in fuel means nothing to something that is not metered in fuel.
    pub fn observe(&mut self, render_ns: u64, fuel: u64) {
        if fuel == 0 || render_ns == 0 {
            return;
        }
        let rate = render_ns as f64 / fuel as f64;
        if self.observations == 0 {
            self.ns_per_fuel = rate;
            self.render_ns = render_ns as f64;
        } else {
            self.ns_per_fuel += (rate - self.ns_per_fuel) * SMOOTHING;
            self.render_ns += (render_ns as f64 - self.render_ns) * SMOOTHING;
        }
        self.observations = self.observations.saturating_add(1);
        self.window_blocks = self.window_blocks.saturating_add(1);
        if render_ns as f64 > self.late_ns() {
            self.window_over = self.window_over.saturating_add(1);
        }
    }

    /// Asked once per block. Returns a budget only when the plugin should be
    /// told about it, which is rarely. `may_raise` is false while the
    /// instrument is being played: a raise is audible, a cut is the
    /// alternative to an xrun, and only one of those may wait.
    /// What the last decision was made on: the share of the window's blocks
    /// that ran late, and the render average against the allowance. Reported
    /// beside the budget, because three rounds of guessing at why a ramp that
    /// missed nothing was still being cut is three rounds too many.
    pub const fn last_window(&self) -> (f64, f64) {
        (self.last_over_rate, self.last_load)
    }

    /// The raw counts the last decision was made on: blocks in the window,
    /// and how many of them ran late. A rate of one means "every block",
    /// which reads the same whether the window held seven hundred blocks or
    /// one -- and those are very different faults.
    pub const fn last_counts(&self) -> (u32, u32) {
        (self.last_blocks, self.last_over)
    }

    pub fn poll(&mut self, now: Duration, may_raise: bool) -> Option<(u64, BudgetReason)> {
        if self.seed_pending {
            self.seed_pending = false;
            self.last_publish = Some(now);
            self.last_window_at = Some(now);
            self.discard_next_window = true;
            return self.published.map(|budget| (budget, BudgetReason::Seeded));
        }
        let rate = self.nanoseconds_per_fuel()?;
        let allowance = self.allowance_ns();
        if allowance <= 0.0 {
            return None;
        }

        let Some(published) = self.published else {
            // Nothing has been measured about how this plugin behaves under a
            // budget yet, so the first one is the only feed-forward step in
            // the whole mechanism: what the allowance buys at the rate this
            // machine has been running at.
            let first = allowance / rate;
            return self.publish_if_credible(first, BudgetReason::Measured, now);
        };

        if let Some(last) = self.last_window_at {
            if now.saturating_sub(last) < PUBLISH_INTERVAL {
                return None;
            }
        }
        // Past the interval: this window is being read, whatever comes of
        // it, so the next one starts here.
        self.last_window_at = Some(now);
        let blocks = self.window_blocks;
        let over = self.window_over;
        self.window_blocks = 0;
        self.window_over = 0;
        if self.discard_next_window {
            // Whatever this window held, the plugin spent it rebuilding to
            // the budget it was just given. Start judging from the next.
            self.discard_next_window = false;
            self.last_publish = Some(now);
            return None;
        }
        if blocks == 0 {
            return None;
        }

        let over_rate = f64::from(over) / f64::from(blocks);
        let load = self.render_ns / allowance;
        self.last_over_rate = over_rate;
        self.last_load = load;
        self.last_blocks = blocks;
        self.last_over = over;
        if over_rate > OVER_TOLERANCE {
            // How far the budget falls follows how far over the render is --
            // but a window can be over on its tail alone, with a mean that
            // looks fine, so every cut is a real cut.
            self.comfortable_since = None;
            if self.exhausted {
                return None;
            }
            // And it has to still be over a window later. See
            // `CONFIRM_WINDOWS`: a chord fills one window with late blocks
            // and leaves the next clean, and cutting on that costs quality
            // for a burst the cut does not shrink.
            self.late_streak = self.late_streak.saturating_add(1);
            if self.late_streak < CONFIRM_WINDOWS {
                return None;
            }
            // Measured against where this streak of cuts started, not
            // against the cut before it. Block times wander by a few percent
            // on their own, so comparing consecutive windows let noise reset
            // the count: on a Raspberry Pi that turned three cuts into nine,
            // and every one of them rebuilt the soundboard.
            if self.load_at_cut.is_none() {
                self.before_streak = self.published;
            }
            if let Some(streak_started_at) = self.load_at_cut {
                if load >= streak_started_at * CUT_MUST_BUY {
                    self.stubborn += 1;
                    if self.stubborn >= STUBBORN_LIMIT {
                        self.exhausted = true;
                        self.last_publish = Some(now);
                        return Some((published, BudgetReason::Exhausted));
                    }
                } else {
                    // Real progress: this is the new baseline to beat, and
                    // the budget that produced it is worth keeping.
                    self.stubborn = 0;
                    self.load_at_cut = Some(load);
                    self.before_streak = self.published;
                }
            }
            let cut = (1.0 / load.max(1.0)).clamp(MOST_SEVERE_CUT, GENTLEST_CUT);
            let published =
                self.publish_if_credible(published as f64 * cut, BudgetReason::Tightened, now);
            if published.is_some() && self.load_at_cut.is_none() {
                // Only a cut that reached the plugin is one it can be judged
                // on. Recording an intention would count a budget the plugin
                // never saw as a budget it ignored.
                self.load_at_cut = Some(load);
            }
            return published;
        }

        self.late_streak = 0;
        if load >= COMFORTABLE || !may_raise {
            // Inside its allowance but not comfortably -- or comfortably,
            // but with someone playing. Leave it exactly where it is: the
            // first is the band the mechanism aims for, the second is a
            // change the player would hear.
            self.comfortable_since = None;
            return None;
        }
        let since = *self.comfortable_since.get_or_insert(now);
        if now.saturating_sub(since) < RAISE_AFTER {
            return None;
        }
        self.comfortable_since = None;
        // Room again: whatever made it stop responding is over.
        self.exhausted = false;
        self.stubborn = 0;
        self.load_at_cut = None;
        self.before_streak = None;
        self.publish_if_credible(published as f64 * RAISE_BY, BudgetReason::Relaxed, now)
    }

    fn publish_if_credible(
        &mut self,
        budget: f64,
        reason: BudgetReason,
        now: Duration,
    ) -> Option<(u64, BudgetReason)> {
        if !budget.is_finite() || budget < MINIMUM_CREDIBLE_BUDGET as f64 {
            return None;
        }
        let budget = budget as u64;
        if let Some(published) = self.published {
            let change = (budget as f64 - published as f64).abs() / published as f64;
            if change < MATERIAL_CHANGE {
                return None;
            }
        }
        self.published = Some(budget);
        self.last_publish = Some(now);
        self.discard_next_window = true;
        Some((budget, reason))
    }
}

/// What one machine settled on, per plugin and period, across sessions.
///
/// Read on the control thread when a voice is built, written on the
/// telemetry thread when a budget settles. The audio thread touches none of
/// it: a voice carries the handful of `(deadline, fuel)` pairs for its plugin
/// by value, and scans them without a lock on its first block.
#[derive(Default)]
pub struct Store {
    path: Option<PathBuf>,
    /// plugin id -> (deadline_ns -> fuel)
    entries: HashMap<String, HashMap<u64, u64>>,
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| Mutex::new(Store::default()))
}

/// Loads the store from `path`, or starts an empty one there. A file that
/// does not parse is treated as empty: a corrupt store must not stop the
/// engine, it just learns again.
pub fn load_store(path: &Path) {
    let mut guard = store().lock().unwrap_or_else(|poison| poison.into_inner());
    guard.path = Some(path.to_path_buf());
    guard.entries.clear();
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    // One line per entry: `<plugin id> <deadline_ns> <fuel>`. Plain enough
    // to read in a terminal and to write without pulling a JSON crate into
    // the engine's real-time binary.
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(plugin), Some(deadline), Some(fuel)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if let (Ok(deadline), Ok(fuel)) = (deadline.parse::<u64>(), fuel.parse::<u64>()) {
            guard
                .entries
                .entry(plugin.to_owned())
                .or_default()
                .insert(deadline, fuel);
        }
    }
}

/// Everything remembered for one plugin, by value, for a voice to carry.
pub fn remembered(plugin: &str) -> Vec<(u64, u64)> {
    let guard = store().lock().unwrap_or_else(|poison| poison.into_inner());
    guard
        .entries
        .get(plugin)
        .map(|by_deadline| by_deadline.iter().map(|(d, f)| (*d, *f)).collect())
        .unwrap_or_default()
}

/// Records a settled budget and rewrites the store. Called from the
/// telemetry thread, never from a render thread.
pub fn remember(plugin: &str, deadline_ns: u64, fuel: u64) {
    let mut guard = store().lock().unwrap_or_else(|poison| poison.into_inner());
    guard
        .entries
        .entry(plugin.to_owned())
        .or_default()
        .insert(deadline_ns, fuel);
    let Some(path) = guard.path.clone() else {
        return;
    };
    let mut text = String::from(
        "# RackForge real-time budgets this machine settled on: plugin, period in ns, fuel per call.\n",
    );
    let mut plugins: Vec<_> = guard.entries.iter().collect();
    plugins.sort_by(|a, b| a.0.cmp(b.0));
    for (plugin, by_deadline) in plugins {
        let mut rows: Vec<_> = by_deadline.iter().collect();
        rows.sort();
        for (deadline, fuel) in rows {
            text.push_str(&format!("{plugin} {deadline} {fuel}\n"));
        }
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 128 frames at 48 kHz, which is what the appliance runs.
    const DEADLINE_NS: u64 = 128 * 1_000_000_000 / 48_000;

    fn governor(slot_count: usize) -> BudgetGovernor {
        let mut governor = BudgetGovernor::default();
        governor.configure(DEADLINE_NS, slot_count);
        governor
    }

    /// Consumes the window after a publish -- which the governor throws away
    /// while the plugin rebuilds to what it was just given -- and returns the
    /// clock at which judging resumes.
    fn grace(governor: &mut BudgetGovernor, published_at: Duration) -> Duration {
        let clock = published_at + PUBLISH_INTERVAL;
        settled(governor, 1_000, 1_000);
        assert_eq!(
            governor.poll(clock, true),
            None,
            "the window after a publish is not judged"
        );
        clock
    }

    /// Runs the window that `CONFIRM_WINDOWS` asks for before a cut: the
    /// blocks are still late, and the governor is expected to say nothing
    /// about them yet. Returns the clock at which it will act.
    fn confirm(
        governor: &mut BudgetGovernor,
        clock: Duration,
        render_ns: u64,
        fuel: u64,
    ) -> Duration {
        let next = clock + PUBLISH_INTERVAL;
        assert_eq!(
            governor.poll(next, true),
            None,
            "one window of lateness is a burst, not a verdict"
        );
        // A poll empties the window it reads, so the next one needs blocks
        // of its own before there is anything to judge.
        settled(governor, render_ns, fuel);
        next
    }

    /// Feeds enough identical blocks for the estimates to settle.
    fn settled(governor: &mut BudgetGovernor, render_ns: u64, fuel: u64) {
        for _ in 0..MINIMUM_OBSERVATIONS * 8 {
            governor.observe(render_ns, fuel);
        }
    }

    #[test]
    fn an_unmetered_plugin_is_never_handed_a_budget() {
        // A native plugin reports no fuel. A budget denominated in fuel means
        // nothing to it, and a number it cannot interpret is worse than none.
        let mut governor = governor(1);
        for _ in 0..10_000 {
            governor.observe(1_000_000, 0);
        }
        assert_eq!(governor.poll(Duration::from_secs(60), true), None);
        assert_eq!(governor.published(), None);
    }

    #[test]
    fn a_governor_that_has_not_been_told_the_period_says_nothing() {
        // Voices are built before the audio loop opens the device. A budget
        // handed out against a guessed deadline is the hardcoded constant this
        // exists to replace, with more steps.
        let mut governor = BudgetGovernor::default();
        settled(&mut governor, 1_000, 1_000);
        assert_eq!(governor.poll(Duration::from_secs(60), true), None);
        governor.configure(DEADLINE_NS, 1);
        assert!(governor.poll(Duration::from_secs(60), true).is_some());
    }

    #[test]
    fn it_waits_for_enough_blocks_before_believing_the_rate() {
        let mut governor = governor(1);
        for _ in 0..MINIMUM_OBSERVATIONS - 1 {
            governor.observe(1_000_000, 1_000_000);
        }
        assert_eq!(governor.poll(Duration::ZERO, true), None);
        governor.observe(1_000_000, 1_000_000);
        assert!(governor.poll(Duration::ZERO, true).is_some());
    }

    #[test]
    fn the_first_budget_is_the_allowance_at_the_measured_rate() {
        // One nanosecond per fuel makes the arithmetic readable: the slot may
        // spend its share of the headroom, in nanoseconds, as fuel.
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (budget, reason) = governor.poll(Duration::ZERO, true).expect("measured");
        assert_eq!(reason, BudgetReason::Measured);
        let expected = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;
        assert!(
            budget.abs_diff(expected) <= expected / 100,
            "{budget} is not within a percent of {expected}"
        );
    }

    #[test]
    fn a_slower_machine_is_handed_a_smaller_budget() {
        // The whole point, stated as a test: same plugin, same work, two
        // machines. The faster one may spend more of itself per block.
        let mut fast = governor(1);
        settled(&mut fast, 1_000_000, 10_000_000);
        let mut slow = governor(1);
        settled(&mut slow, 4_000_000, 10_000_000);
        let (fast_budget, _) = fast
            .poll(Duration::ZERO, true)
            .expect("fast machine measured");
        let (slow_budget, _) = slow
            .poll(Duration::ZERO, true)
            .expect("slow machine measured");
        assert!(
            fast_budget > slow_budget * 3,
            "a machine four times faster was handed {fast_budget} against {slow_budget}"
        );
    }

    #[test]
    fn slots_share_the_headroom_rather_than_each_claiming_it() {
        // Four plugins in a rack cannot each be told they may have the whole
        // machine, or the rack is over budget by construction.
        let mut alone = governor(1);
        settled(&mut alone, 1_000, 1_000);
        let mut crowded = governor(4);
        settled(&mut crowded, 1_000, 1_000);
        let (alone, _) = alone.poll(Duration::ZERO, true).expect("measured");
        let (crowded, _) = crowded.poll(Duration::ZERO, true).expect("measured");
        assert!(alone.abs_diff(crowded * 4) <= alone / 100);
    }

    #[test]
    fn a_plugin_whose_cost_model_is_wrong_still_converges() {
        // The reason the loop is closed. This plugin ignores two thirds of
        // whatever it is told and renders at three times its allowance; no
        // feed-forward arithmetic could have predicted that, and it still has
        // to end up inside the period.
        let mut governor = governor(1);
        // What the governor promises is that blocks stop arriving late, not
        // that they land on the allowance: the allowance sizes the first guess
        // and decides when quality may come back.
        let late = DEADLINE_NS as f64 * LATE_AT;
        settled(&mut governor, 1_000, 1_000);
        let (mut budget, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let overspend = 3.0;

        let mut clock = Duration::ZERO;
        for _ in 0..40 {
            clock += PUBLISH_INTERVAL;
            // What the plugin actually does with the budget it was given.
            let render = (budget as f64 * overspend) as u64;
            settled(&mut governor, render.max(1), budget.max(1));
            match governor.poll(clock, true) {
                Some((next, BudgetReason::Exhausted)) => {
                    panic!("gave up at {next} while cuts were still working")
                }
                Some((next, _)) => budget = next,
                None => {}
            }
            if budget as f64 * overspend <= late {
                break;
            }
        }
        assert!(
            budget as f64 * overspend <= late,
            "a budget of {budget} still renders {} against a deadline that is late at {late}",
            budget as f64 * overspend
        );
    }

    #[test]
    fn lateness_that_lasts_cuts_the_budget() {
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (first, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let base = grace(&mut governor, Duration::ZERO);
        // Every block now runs at twice its allowance, and keeps doing it.
        let allowance = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;
        settled(&mut governor, allowance * 2, 1_000_000);
        let clock = confirm(&mut governor, base, allowance * 2, 1_000_000);
        let (tightened, reason) = governor
            .poll(clock + PUBLISH_INTERVAL, true)
            .expect("lateness that is still there a window later is acted on");
        assert_eq!(reason, BudgetReason::Tightened);
        assert!(tightened < first);
    }

    /// The measurement this rule came from, as a test.
    ///
    /// Twelve notes struck together put half a second of late blocks into one
    /// window and nothing into the next; the same twelve notes HELD cost half
    /// the period and miss nothing. The old law cut on that first window and
    /// thinned the instrument for good -- and, measured on the appliance, the
    /// cut did not shrink the burst at all: 271 misses with no governor
    /// against 225 and 251 with one that had already cut twice.
    #[test]
    fn a_burst_in_one_window_is_not_a_verdict() {
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (first, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let base = grace(&mut governor, Duration::ZERO);
        let allowance = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;
        // One window of a struck chord: most of it late.
        settled(&mut governor, allowance * 2, 1_000_000);
        let clock = confirm(&mut governor, base, allowance / 2, 1_000);
        // And then the notes are just held, which this instrument fits --
        // `confirm` already fed that window.
        assert_eq!(
            governor.poll(clock + PUBLISH_INTERVAL, false),
            None,
            "a burst that has passed must not cost quality"
        );
        assert_eq!(
            governor.settled(clock + PUBLISH_INTERVAL),
            None,
            "and nothing was published to settle on"
        );
        // The budget the plugin holds is the one it started with.
        assert_eq!(
            governor.last_counts().1,
            0,
            "the confirming window was clean"
        );
        let _ = first;
    }

    #[test]
    fn a_rare_late_block_is_tolerated() {
        // One hiccup in a window is a scheduler, not an instrument. Chasing it
        // would leave every machine permanently thinner than it needs to be.
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (first, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let allowance = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;
        for index in 0..1_000 {
            governor.observe(if index == 500 { allowance * 3 } else { 1_000 }, 1_000);
        }
        assert_eq!(governor.poll(PUBLISH_INTERVAL, true), None);
        assert_eq!(governor.published(), Some(first));
    }

    #[test]
    fn quality_comes_back_only_after_a_long_comfortable_stretch() {
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (first, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let base = grace(&mut governor, Duration::ZERO);
        let allowance = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;

        settled(&mut governor, allowance * 2, 1_000_000);
        let cut_at = confirm(&mut governor, base, allowance * 2, 1_000_000);
        let (tightened, _) = governor
            .poll(cut_at + PUBLISH_INTERVAL, true)
            .expect("tightened");
        assert!(tightened < first);

        // Comfortable from here on, but nothing happens for a long time: a
        // budget that tracked every lull would make the timbre breathe.
        let mut clock = cut_at + PUBLISH_INTERVAL;
        for _ in 0..8 {
            clock += PUBLISH_INTERVAL;
            settled(&mut governor, allowance / 4, 1_000_000);
            assert_eq!(
                governor.poll(clock, true),
                None,
                "gave quality back too early"
            );
        }
        clock += RAISE_AFTER;
        settled(&mut governor, allowance / 4, 1_000_000);
        let (relaxed, reason) = governor
            .poll(clock, true)
            .expect("a slot comfortable for long enough may have its quality back");
        assert_eq!(reason, BudgetReason::Relaxed);
        assert!(relaxed > tightened);
    }

    #[test]
    fn a_slot_inside_its_allowance_but_not_comfortable_is_left_alone() {
        // The band the whole mechanism aims for. Nudging here would be churn.
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (first, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let allowance = DEADLINE_NS as f64 * DEFAULT_HEADROOM;
        let mut clock = Duration::ZERO;
        for _ in 0..40 {
            clock += PUBLISH_INTERVAL;
            settled(&mut governor, (allowance * 0.85) as u64, 1_000_000);
            assert_eq!(governor.poll(clock, true), None);
        }
        assert_eq!(governor.published(), Some(first));
    }

    #[test]
    fn a_window_with_no_blocks_in_it_says_nothing() {
        // A stopped stream is not a comfortable one. Reading an empty window
        // as "well inside its allowance" would hand quality back to a slot
        // that has not rendered anything since the last time it was asked.
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (first, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let mut clock = Duration::ZERO;
        for _ in 0..40 {
            clock += PUBLISH_INTERVAL + RAISE_AFTER;
            assert_eq!(governor.poll(clock, true), None);
        }
        assert_eq!(governor.published(), Some(first));
    }

    #[test]
    fn cutting_stops_when_it_stops_buying_anything() {
        // A plugin that ignores the budget, or one already at the floor of
        // what it can do, must not be cut forever. Before this, a Raspberry Pi
        // took Concert Grand from 340,068 fuel to 5,649 in sixteen seconds
        // while the render got slower, because every cut was read as evidence
        // that another was needed.
        let mut governor = governor(1);
        let allowance = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;
        settled(&mut governor, 1_000, 1_000);
        governor.poll(Duration::ZERO, true).expect("measured");

        let mut clock = Duration::ZERO;
        let mut exhausted = None;
        let mut cuts = 0;
        for _ in 0..40 {
            clock += PUBLISH_INTERVAL;
            // Stubbornly over, whatever it is told.
            settled(&mut governor, allowance * 2, 1_000_000);
            match governor.poll(clock, true) {
                Some((budget, BudgetReason::Exhausted)) => {
                    exhausted = Some(budget);
                    break;
                }
                Some((_, BudgetReason::Tightened)) => cuts += 1,
                _ => {}
            }
        }
        let floor = exhausted.expect("the governor never gave up");
        assert!(
            cuts <= STUBBORN_LIMIT + 1,
            "it cut {cuts} times before giving up"
        );

        // And it stays given up rather than churning.
        for _ in 0..10 {
            clock += PUBLISH_INTERVAL;
            settled(&mut governor, allowance * 2, 1_000_000);
            assert_eq!(governor.poll(clock, true), None);
        }
        assert_eq!(governor.published(), Some(floor));

        // Until the machine is free again, at which point it is worth trying.
        // The comfortable stretch is counted from the first poll that sees it,
        // so this takes two: one to start the clock and one to read it.
        clock += PUBLISH_INTERVAL;
        settled(&mut governor, allowance / 4, 1_000_000);
        assert_eq!(
            governor.poll(clock, true),
            None,
            "quality came back immediately"
        );
        clock += RAISE_AFTER;
        settled(&mut governor, allowance / 4, 1_000_000);
        let (_, reason) = governor
            .poll(clock, true)
            .expect("a free machine may try again");
        assert_eq!(reason, BudgetReason::Relaxed);
    }

    #[test]
    fn quality_does_not_come_back_while_the_instrument_is_played() {
        // The player's complaint, as a test: a cut during a dense passage
        // must not be undone twenty seconds later in the middle of the next
        // phrase. It comes back only once the instrument has been silent.
        let mut governor = governor(1);
        let allowance = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;
        settled(&mut governor, 1_000, 1_000);
        governor.poll(Duration::ZERO, true).expect("measured");
        let base = grace(&mut governor, Duration::ZERO);
        settled(&mut governor, allowance * 2, 1_000_000);
        let cut_at = confirm(&mut governor, base, allowance * 2, 1_000_000);
        let (cut, _) = governor
            .poll(cut_at + PUBLISH_INTERVAL, true)
            .expect("tightened");

        // Comfortable, but played: nothing may change, however long it lasts.
        let mut clock = cut_at + PUBLISH_INTERVAL;
        for _ in 0..30 {
            clock += RAISE_AFTER;
            settled(&mut governor, allowance / 4, 1_000_000);
            assert_eq!(governor.poll(clock, false), None, "raised while played");
        }
        assert_eq!(governor.published(), Some(cut));

        // Silent: the same comfort is now allowed to count.
        clock += PUBLISH_INTERVAL;
        settled(&mut governor, allowance / 4, 1_000_000);
        assert_eq!(governor.poll(clock, true), None);
        clock += RAISE_AFTER;
        settled(&mut governor, allowance / 4, 1_000_000);
        let (_, reason) = governor.poll(clock, true).expect("raised in silence");
        assert_eq!(reason, BudgetReason::Relaxed);
    }

    #[test]
    fn a_seeded_budget_lands_on_the_first_block() {
        // Nothing has been measured yet -- the instrument has not rendered a
        // single block -- and the plugin is still told what this machine
        // settled on last time, so it is built right before the first note.
        let mut governor = governor(1);
        governor.seed(2_000_000);
        assert_eq!(
            governor.poll(Duration::ZERO, false),
            Some((2_000_000, BudgetReason::Seeded))
        );
        assert_eq!(governor.poll(Duration::ZERO, false), None);
        assert_eq!(governor.published(), Some(2_000_000));
    }

    #[test]
    fn a_seeded_budget_is_still_cut_when_the_machine_proves_it_must_be() {
        let mut governor = governor(1);
        governor.seed(2_000_000);
        governor.poll(Duration::ZERO, false);
        let base = grace(&mut governor, Duration::ZERO);
        let allowance = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;
        settled(&mut governor, allowance * 2, 1_000_000);
        let cut_at = confirm(&mut governor, base, allowance * 2, 1_000_000);
        let (cut, reason) = governor
            .poll(cut_at + PUBLISH_INTERVAL, false)
            .expect("a seed is a start, not a pin");
        assert_eq!(reason, BudgetReason::Tightened);
        assert!(cut < 2_000_000);
    }

    #[test]
    fn a_budget_settles_when_it_stops_moving_or_when_cutting_gives_up() {
        let mut stubborn = governor(1);
        let mut quiet = governor(1);
        settled(&mut quiet, 1_000, 1_000);
        let (first, _) = quiet.poll(Duration::ZERO, true).expect("measured");
        assert_eq!(quiet.settled(Duration::from_secs(5)), None);
        assert_eq!(quiet.settled(SETTLE_AFTER), Some(first));

        settled(&mut stubborn, 1_000, 1_000);
        stubborn.poll(Duration::ZERO, true).expect("measured");
        let allowance = (DEADLINE_NS as f64 * DEFAULT_HEADROOM) as u64;
        let mut clock = Duration::ZERO;
        let mut gave_up = None;
        for _ in 0..40 {
            clock += PUBLISH_INTERVAL;
            settled(&mut stubborn, allowance * 2, 1_000_000);
            if let Some((budget, BudgetReason::Exhausted)) = stubborn.poll(clock, true) {
                gave_up = Some(budget);
                break;
            }
        }
        // Exhausted settles at once -- but at what was running before the
        // streak, not at the floor those useless cuts reached. See
        // `a_floor_that_cutting_never_earned_is_not_remembered`.
        let remembered = stubborn
            .settled(clock)
            .expect("exhausted is settled at once");
        assert!(
            remembered >= gave_up.expect("it gave up"),
            "remembered {remembered}, the floor rather than what was working"
        );
    }

    #[test]
    fn the_store_round_trips_and_survives_a_corrupt_file() {
        let directory = std::env::temp_dir().join(format!(
            "rackforge-budget-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let path = directory.join("realtime-budget.txt");
        load_store(&path);
        assert!(remembered("org.example.piano").is_empty());
        remember("org.example.piano", DEADLINE_NS, 4_000_000);
        remember("org.example.piano", DEADLINE_NS * 4, 16_000_000);
        load_store(&path);
        let mut rows = remembered("org.example.piano");
        rows.sort();
        assert_eq!(
            rows,
            vec![(DEADLINE_NS, 4_000_000), (DEADLINE_NS * 4, 16_000_000)]
        );

        std::fs::write(&path, "this is not a store").unwrap();
        load_store(&path);
        assert!(remembered("org.example.piano").is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_cuts_own_rebuild_does_not_justify_the_next_cut() {
        // After a cut the plugin rebuilds, and the rebuild is the heaviest
        // thing it does. That window is the governor's own doing and must
        // not be read as the machine asking for more.
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        governor.poll(Duration::ZERO, true).expect("measured");
        let late = (DEADLINE_NS as f64 * LATE_AT) as u64 + 1;
        // Something changed for real: cut once.
        let started = grace(&mut governor, Duration::ZERO);
        settled(&mut governor, late, 1_000_000);
        let mut clock = confirm(&mut governor, started, late, 1_000_000) + PUBLISH_INTERVAL;
        let (first, reason) = governor.poll(clock, true).expect("a real change is cut");
        assert_eq!(reason, BudgetReason::Tightened);
        // The next window is all rebuild -- late blocks the cut caused.
        clock += PUBLISH_INTERVAL;
        settled(&mut governor, late, 1_000_000);
        assert_eq!(
            governor.poll(clock, true),
            None,
            "cut again on its own rebuild"
        );
        assert_eq!(governor.published(), Some(first));
        // And a quiet window after that is quiet: no further cut.
        clock += PUBLISH_INTERVAL;
        settled(&mut governor, 1_000, 1_000_000);
        assert_eq!(governor.poll(clock, true), None);
        assert_eq!(governor.published(), Some(first));
    }

    #[test]
    fn a_render_that_misses_nothing_is_never_cut() {
        // The appliance's own ramp, as a test: twelve held notes at 52 % of
        // the period, a p99 at 84 % of it, and not one deadline lost. Earlier
        // versions cut three times through exactly this.
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        governor.poll(Duration::ZERO, true).expect("measured");
        let mut clock = grace(&mut governor, Duration::ZERO);
        let published = governor.published();
        for _ in 0..40 {
            clock += PUBLISH_INTERVAL;
            // A window of ordinary blocks with the occasional expensive one:
            // a note-on runs the strike simulation and costs more, and it is
            // still comfortably inside the period.
            for index in 0..800 {
                let render = if index % 19 == 0 {
                    (DEADLINE_NS as f64 * 0.84) as u64
                } else {
                    (DEADLINE_NS as f64 * 0.52) as u64
                };
                governor.observe(render, 1_000_000);
            }
            assert_eq!(
                governor.poll(clock, false),
                None,
                "cut a render that missed nothing"
            );
        }
        assert_eq!(governor.published(), published);
    }

    #[test]
    fn a_floor_that_cutting_never_earned_is_not_remembered() {
        // The ratchet, as a test. A plugin at its own minimum cannot answer a
        // smaller budget, so the governor cuts, gives up -- and what it
        // remembers must be what was running before that streak, or the next
        // session starts from the floor and cuts again from there.
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (first, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let mut clock = grace(&mut governor, Duration::ZERO);

        let stuck = (DEADLINE_NS as f64 * 1.4) as u64;
        let mut floor = first;
        for _ in 0..40 {
            clock += PUBLISH_INTERVAL;
            settled(&mut governor, stuck, 1_000_000);
            match governor.poll(clock, true) {
                Some((budget, BudgetReason::Exhausted)) => {
                    assert!(budget <= floor);
                    break;
                }
                Some((budget, _)) => floor = budget,
                None => {}
            }
        }
        let remembered = governor.settled(clock).expect("something to remember");
        assert!(
            remembered >= first,
            "remembered {remembered}, the floor the useless cuts reached, not the {first} that was running"
        );
    }

    #[test]
    fn a_window_that_publishes_nothing_still_starts_the_next_one() {
        // The appliance's own bug, as a test. A window read but not acted on
        // used to leave the cadence timer where it was, so the very next
        // block passed the interval and was judged alone -- and one late
        // block out of one is a hundred percent, which is a cut. Measured:
        // `blocks=1 late=1` on a ramp that missed no deadline at all.
        let mut governor = governor(1);
        settled(&mut governor, 1_000, 1_000);
        let (first, _) = governor.poll(Duration::ZERO, true).expect("measured");
        let mut clock = grace(&mut governor, Duration::ZERO);

        // A window comfortably inside the allowance: read, nothing to say.
        clock += PUBLISH_INTERVAL;
        settled(&mut governor, 1_000, 1_000_000);
        assert_eq!(governor.poll(clock, false), None);

        // The next block is a single expensive one -- a note-on, a rebuild.
        // It must not be a window of its own.
        clock += Duration::from_millis(3);
        governor.observe(DEADLINE_NS * 2, 1_000_000);
        assert_eq!(
            governor.poll(clock, false),
            None,
            "one late block became a whole window"
        );
        assert_eq!(governor.published(), Some(first));
    }

    #[test]
    fn a_machine_too_slow_to_render_anything_is_told_nothing() {
        // Handing over a budget of forty fuel would have the plugin render
        // silence and call it quality. Leaving it alone keeps the audio, and
        // the deadline misses in the log say what is actually wrong.
        let mut governor = governor(1);
        settled(&mut governor, 1_000_000, 1);
        assert_eq!(governor.poll(Duration::from_secs(60), true), None);
        assert_eq!(governor.published(), None);
    }
}
