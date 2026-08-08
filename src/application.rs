//! The consumer-facing application trait — the engine's inversion-of-control seam.
//!
//! The engine owns the window and event loop (it has to: winit's wasm backend
//! throws control flow at the browser and forbids blocking the main thread — see
//! `ARCHITECTURE.md`). So decoupling is achieved by *inversion of control*: a
//! consumer implements [`Application`] and the engine calls *into* it. The
//! engine never sees anything more than `dyn Application`.

use crate::config::Config;
use crate::renderer::Renderer;

/// A consumer of the engine.
///
/// Implement this and hand an instance to [`crate::run`]. The engine drives the
/// window, GPU, and event loop and calls these hooks at the right moments;
/// implementors never touch `wgpu` or `winit`.
///
/// The per-call context is a [`&mut Renderer`](Renderer), whose public API hides
/// the GPU plumbing. (As the engine grows — driveable camera, input — this
/// context is the natural place to expand, and may earn a dedicated name then.)
pub trait Application {
    /// Called once, before the window is created, to describe the window this
    /// consumer wants. The default is [`Config::default`] — a 1280x720 window
    /// titled "SLMSTTAA".
    ///
    /// This is read exactly once, at startup: it configures the window rather
    /// than driving it, so returning a different value later has no effect.
    ///
    /// ```
    /// # use slmsttaa::{Application, Config, Renderer};
    /// # struct Demo;
    /// impl Application for Demo {
    ///     fn config(&self) -> Config {
    ///         Config::default().with_title("Terrain")
    ///     }
    ///
    ///     fn init(&mut self, _renderer: &mut Renderer) {}
    /// }
    /// ```
    fn config(&self) -> Config {
        Config::default()
    }

    /// Called once, after the renderer exists. Upload initial geometry here.
    fn init(&mut self, renderer: &mut Renderer);

    /// Called zero or more times per frame, before [`Application::update`], to
    /// advance simulation state by a **fixed** `dt`.
    ///
    /// `dt` is always the same number — [`Timeline::step`] — however long the
    /// frame took; a slow frame runs this hook more times rather than with a
    /// bigger step. That is what makes a run reproduce, and it is the difference
    /// between this and [`Renderer::dt`].
    ///
    /// **The contract is "advance simulation state here and nowhere else."** A
    /// consumer that honors it is frame-rate independent and can be paused,
    /// slowed, single-stepped and scrubbed via [`Renderer::time_mut`]; one that
    /// also mutates state in `update` has opted out, and the engine cannot tell.
    /// The engine's guarantee is narrow on purpose: it stops being a source of
    /// wall-clock nondeterminism. It does not make you deterministic.
    ///
    /// Not everything belongs here. Camera work, the draw-list, and the UI are
    /// *rendering*, they should run once per frame whatever the step rate, and
    /// they go in [`Application::update`]. The default does nothing, so a
    /// consumer with no simulation ignores this hook entirely.
    ///
    /// [`Timeline::step`]: crate::time::Timeline::step
    /// [`Renderer::dt`]: Renderer::dt
    /// [`Renderer::time_mut`]: Renderer::time_mut
    fn fixed_update(&mut self, _renderer: &mut Renderer, _dt: f32) {}

    /// Whether the engine should quit when Escape is pressed. Defaults to `true`.
    ///
    /// Escape is the one key the engine interprets for itself, which is a
    /// convenience for a demo and a problem for anything with a UI: Escape is
    /// *the* cancel key, so a consumer with a text field to leave or a dialog to
    /// close needs it back. Return `false` and Escape arrives in
    /// [`Renderer::input`] like every other key — at which point
    /// [`Renderer::request_exit`] is how you quit on whatever you choose instead.
    ///
    /// ```
    /// # use slmsttaa::{Application, Key, Renderer};
    /// # struct Demo;
    /// impl Application for Demo {
    ///     fn init(&mut self, _renderer: &mut Renderer) {}
    ///
    ///     // Escape closes the inspector; Q quits.
    ///     fn quit_on_escape(&self) -> bool {
    ///         false
    ///     }
    ///
    ///     fn update(&mut self, renderer: &mut Renderer) {
    ///         if renderer.input().is_key_pressed(Key::Q) {
    ///             renderer.request_exit();
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// [`Renderer::input`]: Renderer::input
    /// [`Renderer::request_exit`]: Renderer::request_exit
    fn quit_on_escape(&self) -> bool {
        true
    }

    /// Called every frame, just before the engine draws. Build the draw-list, the
    /// UI, and the camera here. The default does nothing.
    ///
    /// Rendering between two fixed steps is what [`Timeline::alpha`] is for — see
    /// its docs. Simulation state belongs in [`Application::fixed_update`].
    ///
    /// [`Timeline::alpha`]: crate::time::Timeline::alpha
    fn update(&mut self, _renderer: &mut Renderer) {}
}
