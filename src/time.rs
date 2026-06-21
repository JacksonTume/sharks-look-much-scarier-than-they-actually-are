//! A minimal, cross-platform frame clock.
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

/// Tracks the timestamp of the previous frame to produce a per-frame delta.
#[derive(Debug)]
pub struct Clock {
    /// Timestamp of the last [`Clock::tick`], in seconds. `None` until the first.
    last: Option<f64>,
    /// The most recent delta, in seconds.
    dt: f32,
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
        }
    }

    /// Advance the clock to "now" and return the elapsed time since the previous
    /// tick, in seconds. The first tick reports `0.0` (no previous frame).
    ///
    /// The delta is clamped to a sane maximum so a long stall (a debugger pause,
    /// a backgrounded tab) can't inject a huge time step into a consumer's
    /// simulation.
    pub fn tick(&mut self) -> f32 {
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
