//! Reproducible frames, for a harness that photographs the engine.
//!
//! # Why this exists
//!
//! `ROADMAP.md`'s Definition of Done ends with "the driving demo runs and shows
//! the new capability on screen", and that is not decoration: six bugs on record
//! passed the whole test suite and were caught by a human looking at a window.
//! Automating that look is worth a lot — and it fails on two things that have
//! nothing to do with graphics.
//!
//! - **A run is not reproducible.** [`Clock`](crate::time::Clock) measures wall
//!   time, so `elapsed` — which drives the ripple field, the `Timeline`'s step
//!   payout and every UI animation — is different on every run. Screenshotting
//!   the same commit twice produced a 0.6% difference in the terrain demo, all
//!   of it in the water, which is enough to drown any real regression.
//! - **A run has no observable moment.** Nothing announces "frame 120 is on
//!   screen", so a harness has no choice but to `sleep` and hope.
//!
//! Both are fixed here, and the fix is deliberately *outside* the public API.
//!
//! # Why environment variables
//!
//! Because the alternative is worse. A `Renderer::set_capture(..)` method would
//! be public surface that exists only for testing, on a type whose whole design
//! rule is that `examples/triangle.rs` must be writable from public items alone.
//! The engine already reads `RUST_LOG` from the environment; this is the same
//! trade, and it means a consumer cannot see that capture mode exists at all.
//!
//! Native only. Env vars are meaningless on `wasm32-unknown-unknown`, and the
//! browser cannot be photographed this way regardless.
//!
//! # The variables
//!
//! - **`SLMSTTAA_CAPTURE_DT`** — seconds. Pins every frame delta to exactly this,
//!   which makes the rendered frame a pure function of the frame *index*.
//! - **`SLMSTTAA_CAPTURE_FRAMES`** — comma-separated frame numbers. On reaching
//!   one, the engine prints `slmsttaa: capture <n>` to **stdout** and freezes;
//!   it resumes when a line arrives on **stdin**.
//!
//! Setting neither leaves the engine exactly as it was: [`Capture::from_env`]
//! returns `None` and every call site short-circuits.
//!
//! # What "freeze" means, and why it is also the input window
//!
//! A frozen frame skips `begin_frame`, `Application::fixed_update`,
//! `Application::update` and `Input::end_frame`, and calls only
//! `Renderer::render`. Three consequences, all load-bearing:
//!
//! - The window **keeps presenting the same picture**, so an external grab has
//!   something to photograph. A blocking read here instead would stop the event
//!   loop and, with no compositor, the framebuffer contents would be whatever the
//!   window system last had.
//! - `begin_frame` is what clears the overlay, so the UI's vertices survive the
//!   freeze rather than the panels vanishing from the screenshot.
//! - `Input::end_frame` is what clears press edges — so a click delivered *while
//!   frozen* still has its edge set when the next real frame runs. The freeze is
//!   an input window for free, which is what lets a harness click a specific
//!   thing on a specific frame.
//!
//! On resume the harness's own pointer *motion* is thrown away
//! ([`Input::discard_motion`](crate::Input)), because warping the cursor across
//! the screen would otherwise arrive as one enormous delta and spin any camera
//! that orbits by it. Press edges and the **wheel** deliberately survive that:
//! both are input a script asked for rather than a side effect of aiming.
//!
//! **The absolute cursor survives too, which is what makes a scripted drag
//! work.** A harness that presses on one checkpoint, moves on the next and
//! releases on a third leaves the button *held* across the real frames in
//! between — button state is a level, not an edge, so nothing here clears it —
//! and a widget that follows `cursor_position` while held therefore tracks it.
//! Only the delta is discarded, which is exactly right: a slider wants to know
//! where the pointer *is*, and a camera wants to know how far it moved.
//!
//! # The one thing a script author has to know
//!
//! A frozen frame runs no `update`, so **anything a consumer derives from the
//! previous frame's update is stale for as long as it is parked**. The sharp
//! case is a demo that asks the UI whether the pointer is over a panel — that
//! answer is a frame old by design (see `examples/editor.rs`'s `ui_pointer`), and
//! while frozen it is older still. Move the pointer on an *earlier* frame than
//! you click or scroll with it, so a real update runs in between. Getting this
//! wrong looks exactly like the feature being broken: the first script written
//! against a scroll area sent its wheel to the camera instead.

use std::sync::mpsc::{self, Receiver, TryRecvError};

/// What a redraw should do this time round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// A normal frame: simulate, build, draw.
    Running,
    /// Held on a checkpoint: re-present the last picture and do nothing else.
    Frozen,
    /// The first frame after a freeze released. Normal, except that the pointer
    /// motion accumulated while parked belongs to the harness, not the user.
    Resumed,
}

/// Frame-level capture control, present only when the environment asks for it.
#[derive(Debug)]
pub(crate) struct Capture {
    /// Pinned frame delta in seconds, or `None` to keep measuring wall time.
    dt: Option<f32>,
    /// Frames to stop on, ascending. Consumed from the front as they are hit.
    checkpoints: Vec<u64>,
    /// Frames completed so far.
    frame: u64,
    /// Whether the engine is currently holding on a checkpoint.
    frozen: bool,
    /// Lines arriving on stdin; one of them releases a freeze.
    resume: Receiver<()>,
}

impl Capture {
    /// Read the environment, or `None` if capture mode was not requested.
    ///
    /// Called once, from `Renderer::new`. A malformed value is a warning and is
    /// then ignored rather than a panic: capture mode is a testing convenience,
    /// and taking down a consumer's application over a typo in an environment
    /// variable it does not know about would be a poor trade.
    pub(crate) fn from_env() -> Option<Self> {
        let dt = parse_dt();
        let checkpoints = parse_checkpoints();
        if dt.is_none() && checkpoints.is_empty() {
            return None;
        }

        log::info!("capture mode: dt={dt:?} checkpoints={checkpoints:?}");
        Some(Self {
            dt,
            checkpoints,
            frame: 0,
            frozen: false,
            resume: spawn_stdin_reader(),
        })
    }

    /// The delta every frame should report, if it is being pinned.
    pub(crate) fn dt(&self) -> Option<f32> {
        self.dt
    }

    /// What this redraw should do, and the place a freeze ends.
    ///
    /// A frozen engine checks for the resume line here, so the check happens once
    /// per redraw and never blocks. The [`Step::Resumed`] case is distinct from
    /// [`Step::Running`] on purpose: exactly one frame per freeze needs to throw
    /// away the pointer motion the harness caused, and telling the caller *which*
    /// frame that is beats making it guess.
    pub(crate) fn step(&mut self) -> Step {
        if !self.frozen {
            return Step::Running;
        }
        match self.resume.try_recv() {
            // A line arrived, or stdin closed. Either way, stop waiting — a
            // closed stdin means nobody is driving us (the app was launched from
            // a terminal, not by the harness) and hanging forever would be the
            // worst possible behaviour.
            Ok(()) | Err(TryRecvError::Disconnected) => {
                self.frozen = false;
                Step::Resumed
            }
            Err(TryRecvError::Empty) => Step::Frozen,
        }
    }

    /// Count a completed frame, and freeze if it was a checkpoint.
    ///
    /// The marker goes to **stdout** rather than through `log`, because the
    /// harness parses it and `RUST_LOG` must not be able to hide it.
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        if self.checkpoints.first() != Some(&self.frame) {
            return;
        }
        self.checkpoints.remove(0);
        self.frozen = true;
        // Flushed explicitly: stdout is block-buffered when it is a pipe, which
        // is exactly the case the harness reads it through, and a marker sitting
        // in a buffer is a harness that waits forever.
        println!("slmsttaa: capture {}", self.frame);
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}

/// `SLMSTTAA_CAPTURE_DT` as positive seconds.
fn parse_dt() -> Option<f32> {
    let raw = std::env::var("SLMSTTAA_CAPTURE_DT").ok()?;
    match raw.trim().parse::<f32>() {
        Ok(dt) if dt > 0.0 && dt.is_finite() => Some(dt),
        _ => {
            log::warn!("SLMSTTAA_CAPTURE_DT={raw:?} is not a positive number; ignoring");
            None
        }
    }
}

/// `SLMSTTAA_CAPTURE_FRAMES` as a sorted, de-duplicated ascending list.
///
/// Sorted because the checkpoint list is consumed from the front, so an
/// out-of-order entry would silently never fire.
fn parse_checkpoints() -> Vec<u64> {
    let Ok(raw) = std::env::var("SLMSTTAA_CAPTURE_FRAMES") else {
        return Vec::new();
    };
    let mut frames: Vec<u64> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| match s.parse::<u64>() {
            Ok(n) if n > 0 => Some(n),
            _ => {
                log::warn!("SLMSTTAA_CAPTURE_FRAMES: ignoring {s:?}");
                None
            }
        })
        .collect();
    frames.sort_unstable();
    frames.dedup();
    frames
}

/// Read stdin on a background thread, signalling once per line.
///
/// A thread rather than a non-blocking read because there is no portable way to
/// poll stdin in std, and because the alternative — reading inline — would stop
/// the event loop and with it the redraws that keep the window photographable.
///
/// The channel is bounded by nothing and that is fine: the harness writes one
/// line per checkpoint, and an extra line just releases the next freeze early.
fn spawn_stdin_reader() -> Receiver<()> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            if line.is_err() || tx.send(()).is_err() {
                break;
            }
        }
        // Dropping `tx` here disconnects the channel, which `frozen` reads as
        // "nobody is driving us" and treats as permission to carry on.
    });
    rx
}
