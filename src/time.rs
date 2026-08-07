//! A minimal, cross-platform frame clock, and the fixed-timestep [`Timeline`]
//! built on top of it.
//!
//! **There are two clocks here, and the split is the point.**
//!
//! [`Clock`] measures the wall-clock time between frames so consumers can drive
//! frame-rate-independent animation (and the engine can report an FPS readout).
//! It exists because timing is the one piece of "obvious plumbing" that diverges
//! sharply between targets:
//!
//! - **Native** uses [`std::time::Instant`].
//! - **Web** must not — `Instant::now()` *panics* on `wasm32-unknown-unknown`.
//!   We read `performance.now()` (high-resolution milliseconds) through `web-sys`
//!   instead.
//!
//! The divergence is isolated to [`Clock::now_seconds`]; the rest of the engine
//! sees a single `tick() -> dt` API (see `ARCHITECTURE.md`, "Input flow" — this is
//! the long-deferred frame clock that finally arrived with the erosion demo).
//!
//! [`Timeline`] sits entirely *above* that — it consumes the wall delta and hands
//! back a whole number of identical steps, which is what makes a consumer's
//! simulation reproducible instead of merely smooth. It touches no platform API,
//! so it needed no `#[cfg]` and could be unit-tested without a GPU or a window.
//!
//! Which one to read:
//!
//! - **Wall time ([`Clock`], `Renderer::dt`)** — anything that should keep moving
//!   while the simulation is paused: an FPS readout, a UI hover fade.
//! - **Simulation time ([`Timeline`], `Application::fixed_update`)** — state that
//!   has to reproduce and has to be pausable.

/// Tracks the timestamp of the previous frame to produce a per-frame delta.
#[derive(Debug)]
pub struct Clock {
    /// Timestamp of the last [`Clock::tick`], in seconds. `None` until the first.
    last: Option<f64>,
    /// The most recent delta, in seconds.
    dt: f32,
    /// A delta to report instead of measuring one — the screenshot harness
    /// pinning time so a frame is a pure function of its index. `None` in every
    /// normal run, which is every run a consumer will ever have.
    pinned: Option<f32>,
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    /// Create a clock that has not yet ticked.
    pub fn new() -> Self {
        Self {
            last: None,
            dt: 0.0,
            pinned: None,
        }
    }

    /// Report `dt` on every tick instead of measuring wall time.
    ///
    /// For the screenshot harness only, and the reason it can diff two runs at
    /// all: with the delta pinned, `elapsed` — and therefore the ripple field,
    /// the [`Timeline`]'s step payout and every UI animation — depends only on
    /// how many frames have run, not on how fast the machine ran them.
    ///
    /// Note the **first tick still reports `0.0`**, pinned or not. That is not an
    /// oversight: a frame with no predecessor genuinely has no delta, and keeping
    /// the rule identical in both modes is what makes the pinned clock a faithful
    /// stand-in rather than a subtly different one.
    ///
    /// Native only — the harness photographs a window, and the browser has none
    /// it can reach. `tick` still consults `pinned` on both targets, where on the
    /// web it is simply always `None`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn pin(&mut self, dt: f32) {
        self.pinned = Some(dt);
    }

    /// Advance the clock to "now" and return the elapsed time since the previous
    /// tick, in seconds. The first tick reports `0.0` (no previous frame).
    ///
    /// The delta is clamped to a sane maximum so a long stall (a debugger pause,
    /// a backgrounded tab) can't inject a huge time step into a consumer's
    /// simulation.
    pub fn tick(&mut self) -> f32 {
        if let Some(dt) = self.pinned {
            // Still gated on `last`, so the first frame reports 0.0 exactly as an
            // unpinned clock does. `last` is set to a non-`None` sentinel rather
            // than a timestamp — nothing reads its value once pinned.
            self.dt = if self.last.is_some() { dt } else { 0.0 };
            self.last = Some(0.0);
            return self.dt;
        }
        let now = Self::now_seconds();
        let dt = match self.last {
            Some(prev) => (now - prev).max(0.0) as f32,
            None => 0.0,
        };
        // Cap at ~100 ms; beyond that we'd rather stutter than explode.
        self.dt = dt.min(0.1);
        self.last = Some(now);
        self.dt
    }

    /// The most recent per-frame delta, in seconds, without advancing the clock.
    pub fn dt(&self) -> f32 {
        self.dt
    }

    /// A monotonic-ish timestamp in seconds, from the platform's best clock.
    #[cfg(not(target_arch = "wasm32"))]
    fn now_seconds() -> f64 {
        use std::sync::OnceLock;
        use std::time::Instant;
        // Anchor to a fixed origin so we return seconds-since-start as an f64.
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        let origin = ORIGIN.get_or_init(Instant::now);
        origin.elapsed().as_secs_f64()
    }

    /// Web timestamp via `performance.now()` (milliseconds → seconds). Falls back
    /// to `0.0` if the API is somehow unavailable, which simply yields `dt = 0`.
    #[cfg(target_arch = "wasm32")]
    fn now_seconds() -> f64 {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now() / 1000.0)
            .unwrap_or(0.0)
    }
}

/// The most fixed steps the engine will run in a single frame.
///
/// [`Clock`] already clamps a stall to 100 ms, which is six steps at the default
/// 60 Hz — but [`Timeline::set_scale`] multiplies that, so the cap is what makes
/// the bound hold whatever the scale. Beyond it the remainder is **dropped**:
/// simulation time falls behind wall time, which is the correct failure. The
/// alternative is the classic spiral, where a frame that ran long schedules even
/// more work for the next one and the program never catches up.
const MAX_STEPS_PER_FRAME: u32 = 8;

/// A fixed-timestep simulation clock the consumer can pause, scale, and scrub.
///
/// [`Clock`] tells you how long the last frame took, which is exactly the wrong
/// thing to advance a simulation by: the step size then depends on the machine,
/// so a run cannot be reproduced and there is no way to express "stop" or "one
/// step, please". `Timeline` accumulates the wall delta and pays it out in whole
/// steps of a fixed size, driving [`Application::fixed_update`] zero or more
/// times per frame.
///
/// [`Application::fixed_update`]: crate::Application::fixed_update
///
/// # What this does and does not guarantee
///
/// The engine's contribution is *narrow but real*: a consumer that advances its
/// state **only** inside the fixed hook sees the same sequence of identical steps
/// regardless of frame rate, so the engine stops being a source of wall-clock
/// nondeterminism. It does **not** make a consumer deterministic — that stays the
/// consumer's job, and a consumer that also mutates state in `update` has opted
/// out.
///
/// # Rendering between steps
///
/// A 60 Hz step on a 144 Hz display means most frames fall *between* steps, and
/// drawing the last completed step on each of them judders. [`Timeline::alpha`]
/// is how far through the pending step the frame is; what to do with it is the
/// consumer's business. A consumer holding two snapshots blends them; one whose
/// pose is a pure function of time (`examples/scene.rs`) simply evaluates that
/// function at `elapsed + alpha * step`.
#[derive(Debug, Clone)]
pub struct Timeline {
    /// The fixed step, in seconds. `1 / rate`.
    step: f32,
    /// Unspent simulation time, always less than one `step` after a frame.
    accumulator: f32,
    /// Total simulation time paid out, in seconds.
    elapsed: f32,
    /// Multiplier applied to the wall delta before accumulating.
    scale: f32,
    /// While set, the wall delta is ignored entirely.
    paused: bool,
    /// Steps requested by [`Timeline::step_once`], honored even while paused.
    queued: u32,
    /// Every step ever paid out. Not derivable from `elapsed`, which a seek moves
    /// without running anything.
    steps: u64,
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    /// A timeline running at 60 steps per second, unpaused, at real time.
    pub fn new() -> Self {
        Self {
            step: 1.0 / 60.0,
            accumulator: 0.0,
            elapsed: 0.0,
            scale: 1.0,
            paused: false,
            queued: 0,
            steps: 0,
        }
    }

    /// Set the fixed step rate, in steps per second (clamped to a sane range).
    ///
    /// Changing this mid-run changes the step size but keeps [`Timeline::elapsed`]
    /// — the clock does not jump, only its granularity changes.
    pub fn set_rate(&mut self, hz: f32) {
        self.step = 1.0 / hz.clamp(1.0, 1000.0);
    }

    /// Steps per second.
    pub fn rate(&self) -> f32 {
        1.0 / self.step
    }

    /// The fixed step, in seconds — the `dt` handed to every `fixed_update`.
    pub fn step(&self) -> f32 {
        self.step
    }

    /// Scale simulation time against wall time. `1.0` is real time, `0.5` is half
    /// speed, `2.0` is double. Negative values are clamped away: running the hook
    /// backwards would hand a consumer a negative `dt`, and nothing can integrate
    /// that safely. To go back, [`Timeline::seek`].
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.max(0.0);
    }

    /// The current time scale.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Stop (or resume) paying out steps.
    ///
    /// While paused the accumulator is left exactly where it is, so
    /// [`Timeline::alpha`] holds still rather than creeping the rendered instant
    /// forward through a step that will never complete.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Whether the timeline is paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Run exactly one step on the next frame, even while paused.
    ///
    /// The accumulator is cleared by the step, so single-stepping lands on a step
    /// boundary (`alpha == 0`) instead of leaving a stale fraction behind that
    /// would offset every subsequent frame's rendered instant.
    pub fn step_once(&mut self) {
        self.queued += 1;
    }

    /// Move the clock to `seconds`, clearing any unspent time.
    ///
    /// **This moves the engine's clock and nothing else.** No catch-up steps are
    /// run and no consumer is rewound — the engine cannot un-erode a landscape or
    /// un-fire an event, so it does not pretend to. A consumer whose state is a
    /// pure function of time keeps its own clock in agreement in one line; a
    /// consumer carrying irreversible state should not offer a scrub control at
    /// all.
    pub fn seek(&mut self, seconds: f32) {
        self.elapsed = seconds.max(0.0);
        self.accumulator = 0.0;
    }

    /// Total simulation time, in seconds — the sum of every step paid out.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// Every fixed step this timeline has paid out since it was created.
    ///
    /// The honest readout for "is this frame-rate independent?", where the *last*
    /// frame's count is not: at 75 Hz against a 60 Hz step the per-frame number
    /// strobes 0,1,1,1,1 forever, so a single sample of it says nothing. This one
    /// is monotone, and two runs of the same wall duration at different frame
    /// rates land on the same value.
    ///
    /// A [`Timeline::seek`] moves [`Timeline::elapsed`] but runs no steps, so
    /// after one the two stop being multiples of each other. That is the point of
    /// keeping both.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// How far the current frame falls through the pending step, in `[0, 1)`.
    ///
    /// See the type docs: this is the number that keeps rendering smooth when the
    /// frame rate and the step rate disagree.
    pub fn alpha(&self) -> f32 {
        (self.accumulator / self.step).clamp(0.0, 1.0)
    }

    /// Accumulate `wall_dt` and return how many fixed steps to run this frame.
    ///
    /// `elapsed` is advanced here rather than by the caller, so the count and the
    /// clock cannot disagree even if a consumer's hook panics or the caller drops
    /// a step.
    pub(crate) fn begin_frame(&mut self, wall_dt: f32) -> u32 {
        if !self.paused {
            self.accumulator += wall_dt * self.scale;
        }

        let mut steps = 0;
        while self.accumulator >= self.step && steps < MAX_STEPS_PER_FRAME {
            self.accumulator -= self.step;
            steps += 1;
        }

        // Dropping the remainder rather than carrying it is what stops a long
        // frame from scheduling an even longer one.
        if self.accumulator >= self.step {
            self.accumulator = 0.0;
        }

        // Manual steps ride on top of the cap and clear the fraction, so a
        // single-step lands exactly on a boundary.
        if self.queued > 0 {
            steps += std::mem::take(&mut self.queued).min(MAX_STEPS_PER_FRAME);
            self.accumulator = 0.0;
        }

        self.elapsed += steps as f32 * self.step;
        self.steps += steps as u64;
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every duration here is a negative power of two, so the accumulator's
    /// arithmetic is exact and these assertions are about the algorithm rather
    /// than about float drift. A 1/60 step and a 1/144 frame would leave the
    /// step count one either side of the boundary at the mercy of rounding.
    const RATE: f32 = 64.0;

    /// A timeline stepping at an exactly-representable rate.
    fn timeline() -> Timeline {
        let mut timeline = Timeline::new();
        timeline.set_rate(RATE);
        timeline
    }

    /// Feed a sequence of frame deltas and count the steps they paid out.
    fn run(timeline: &mut Timeline, frames: &[f32]) -> u32 {
        frames.iter().map(|&dt| timeline.begin_frame(dt)).sum()
    }

    /// The reproducibility claim, and the one most worth a test: the same second
    /// of wall time yields the same steps however it was chopped into frames.
    /// This is the property `Renderer::dt()` cannot offer.
    #[test]
    fn step_count_is_independent_of_frame_pacing() {
        // One second, three ways: 128 even frames, 64 even frames, and a ragged
        // mix of both — all summing to exactly 1.0.
        let smooth = vec![1.0 / 128.0; 128];
        let slow = vec![1.0 / 64.0; 64];
        let mut ragged = vec![1.0 / 64.0; 32];
        ragged.extend(vec![1.0 / 128.0; 64]);

        for frames in [smooth, slow, ragged] {
            let mut timeline = timeline();
            assert_eq!(run(&mut timeline, &frames), RATE as u32);
            assert_eq!(timeline.elapsed(), 1.0);
        }
    }

    #[test]
    fn paused_pays_out_nothing_and_freezes_alpha() {
        let mut timeline = timeline();
        timeline.begin_frame(1.0 / 128.0); // half-way into a step
        let alpha = timeline.alpha();
        assert_eq!(alpha, 0.5);

        timeline.set_paused(true);
        assert_eq!(run(&mut timeline, &[1.0 / 64.0; 10]), 0);
        assert_eq!(timeline.alpha(), alpha, "alpha crept while paused");
        assert_eq!(timeline.elapsed(), 0.0);
    }

    #[test]
    fn step_once_runs_exactly_one_step_while_paused() {
        let mut timeline = timeline();
        timeline.set_paused(true);
        timeline.step_once();

        assert_eq!(timeline.begin_frame(1.0 / 64.0), 1);
        assert_eq!(timeline.elapsed(), timeline.step());
        // ...and lands on a boundary, so the frame after renders the step it ran
        // rather than a fraction past it.
        assert_eq!(timeline.alpha(), 0.0);
        // No repeats: the queue is consumed.
        assert_eq!(timeline.begin_frame(1.0 / 64.0), 0);
    }

    #[test]
    fn alpha_stays_in_range_across_a_sweep() {
        let mut timeline = timeline();
        for i in 0..500 {
            timeline.begin_frame(0.001 + i as f32 * 0.0001);
            let alpha = timeline.alpha();
            assert!((0.0..1.0).contains(&alpha), "alpha was {alpha}");
        }
    }

    /// A stall must not schedule hundreds of steps into one frame — that is the
    /// spiral this cap exists to prevent.
    #[test]
    fn a_long_stall_is_capped_rather_than_spiralling() {
        let mut timeline = timeline();
        assert_eq!(timeline.begin_frame(5.0), MAX_STEPS_PER_FRAME);
        // The dropped remainder is dropped for good — the next frame is normal.
        assert_eq!(timeline.begin_frame(1.0 / 64.0), 1);
    }

    #[test]
    fn scale_stretches_simulation_time() {
        let mut half = timeline();
        half.set_scale(0.5);
        assert_eq!(run(&mut half, &[1.0 / 64.0; 64]), 32);

        let mut double = timeline();
        double.set_scale(2.0);
        assert_eq!(run(&mut double, &[1.0 / 64.0; 64]), 128);
    }

    #[test]
    fn seek_moves_the_clock_and_clears_the_fraction() {
        let mut timeline = timeline();
        run(&mut timeline, &[1.0 / 128.0; 5]);

        timeline.seek(12.5);
        assert_eq!(timeline.elapsed(), 12.5);
        assert_eq!(timeline.alpha(), 0.0);
        // Seeking runs no catch-up steps; the clock moved and nothing else did.
        assert_eq!(timeline.begin_frame(1.0 / 64.0), 1);
        assert_eq!(timeline.elapsed(), 12.5 + timeline.step());
    }

    #[test]
    fn rate_changes_granularity_without_moving_the_clock() {
        let mut timeline = timeline();
        run(&mut timeline, &[1.0 / 64.0; 64]);
        assert_eq!(timeline.elapsed(), 1.0);

        timeline.set_rate(128.0);
        assert_eq!(timeline.elapsed(), 1.0, "the clock jumped on a rate change");
        assert_eq!(run(&mut timeline, &[1.0 / 64.0; 64]), 128);
    }
}
