//! The platform's half of `cargo xtask shoot`.
//!
//! The interesting half of the harness is not platform-specific at all. Pinning
//! the frame clock, announcing checkpoints on stdout and holding there until
//! released is done by the engine (`src/capture.rs`) and driven by [`shoot`] over
//! a pipe, and none of that knows what an X server is.
//!
//! What *is* platform-specific is three things: somewhere for the demo to render,
//! a way to photograph it, and a way to poke it. This module is those three, and
//! nothing else.
//!
//! [`shoot`]: crate::shoot
//!
//! # The two implementations, and why they differ so much
//!
//! - **X11** ([`x11`]) owns an `Xvfb` display and shells out to `import` and
//!   `xdotool`. The virtual display is the good part: a window nobody can see,
//!   at a size that does not depend on the machine, which is what makes two runs
//!   of the same commit pixel-identical.
//! - **Windows** ([`win32`]) has no equivalent, so it does the opposite of
//!   shelling out — it talks to Win32 directly, because this crate has no
//!   dependencies and is going to keep it that way. There is no virtual display,
//!   so the window is a real one; see [`win32`] for what is done to keep that
//!   from being anyone's problem.
//!
//! Both present the same verbs, so [`shoot`] contains no `#[cfg]` at all.

use std::path::{Path, PathBuf};

#[cfg(windows)]
mod png;
#[cfg(windows)]
mod win32;
#[cfg(unix)]
mod x11;

#[cfg(windows)]
use win32 as platform;
#[cfg(unix)]
use x11 as platform;

/// Somewhere to render a demo, photograph it, and click on it.
pub struct Harness {
    inner: platform::Inner,
    root: PathBuf,
}

impl Harness {
    /// Fail early, once, naming everything this platform is missing.
    ///
    /// Called **before** the build, because a three-minute compile followed by
    /// "could not run `Xvfb`" wastes the time and buries the actual reason.
    pub fn require_tools() {
        platform::require_tools();
    }

    /// Prepare a place for the demo to appear, `w` by `h` pixels.
    pub fn start(root: &Path, w: u32, h: u32) -> Self {
        Self {
            inner: platform::start(w, h),
            root: root.to_path_buf(),
        }
    }

    /// Environment the demo must be launched with to land here.
    ///
    /// `DISPLAY` on X11; nothing at all on Windows, where the window is found
    /// after the fact by process id instead of being pointed somewhere.
    pub fn env(&self) -> Vec<(String, String)> {
        platform::env(&self.inner)
    }

    /// Tell the harness which process it is photographing.
    ///
    /// Called once, after spawn. X11 ignores it — the display already contains
    /// exactly one window. Windows needs it, because a desktop contains many.
    pub fn attach(&mut self, pid: u32) {
        platform::attach(&mut self.inner, pid);
    }

    /// Photograph the demo's window into `file`, as a PNG.
    pub fn shot(&mut self, file: &Path) {
        platform::shot(&mut self.inner, &self.root, file);
    }

    /// Move the pointer, in physical pixels from the window's top-left.
    pub fn mouse_move(&mut self, x: u32, y: u32) {
        platform::mouse_move(&mut self.inner, &self.root, x, y);
    }

    /// Hold the left button down wherever the pointer is.
    ///
    /// Paired with [`Harness::release`], and the pair is what lets a script
    /// *drag*. A press and a release delivered inside one frozen frame — which is
    /// what [`Harness::click`] is — never gives the demo a frame with the button
    /// down, so `Response::held` is never true and every drag widget in the
    /// toolkit ignores it. Split across two checkpoints, real frames run in
    /// between and a slider moves.
    pub fn press(&mut self) {
        platform::press(&mut self.inner, &self.root);
    }

    /// Let the left button back up.
    pub fn release(&mut self) {
        platform::release(&mut self.inner, &self.root);
    }

    /// Press and release the left button wherever the pointer is.
    ///
    /// Defined as the pair rather than implemented again per platform, so a
    /// click and a scripted press/release cannot drift apart.
    pub fn click(&mut self) {
        self.press();
        self.release();
    }

    /// Turn the wheel by `notches`, negative to scroll down.
    pub fn wheel(&mut self, notches: i32) {
        platform::wheel(&mut self.inner, &self.root, notches);
    }

    /// Tap a key, named the way a capture script names it (`space`, `Escape`,
    /// `w`) — which is `xdotool`'s vocabulary, kept on both platforms so a
    /// script is not written twice.
    pub fn key(&mut self, name: &str) {
        platform::key(&mut self.inner, &self.root, name);
    }

    /// Tear down whatever [`Harness::start`] built.
    pub fn stop(&mut self) {
        platform::stop(&mut self.inner);
    }
}
