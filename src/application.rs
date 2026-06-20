//! The consumer-facing application trait — the engine's inversion-of-control seam.
//!
//! The engine owns the window and event loop (it has to: winit's wasm backend
//! throws control flow at the browser and forbids blocking the main thread — see
//! `ARCHITECTURE.md`). So decoupling is achieved by *inversion of control*: a
//! consumer implements [`Application`] and the engine calls *into* it. The
//! engine never sees anything more than `dyn Application`.

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
    /// Called once, after the renderer exists. Upload initial geometry here.
    fn init(&mut self, renderer: &mut Renderer);

    /// Called every frame, just before the engine draws. Mutate per-frame state
    /// (geometry, later the camera) here. The default does nothing.
    fn update(&mut self, _renderer: &mut Renderer) {}
}
