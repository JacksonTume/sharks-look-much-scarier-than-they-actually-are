//! # SLMSTTAA — Sharks Look Much Scarier Than They Actually Are
//!
//! A small, approachable Rust 3D rendering engine built on [`wgpu`] and
//! [`winit`]. Despite the intimidating name, the internals are friendlier than
//! they look.
//!
//! The crate is split into a few focused modules:
//!
//! - [`app`]     — windowing + event loop glue (winit `ApplicationHandler`).
//! - [`renderer`] — the wgpu device/surface/pipeline plumbing.
//! - [`camera`]  — a minimal perspective camera producing a view-projection matrix.
//!
//! The simplest way to see something on screen is [`run`], which opens a window
//! and renders the demo scene until you close it.

pub mod app;
pub mod camera;
pub mod renderer;

pub use app::App;
pub use camera::Camera;
pub use renderer::Renderer;

use winit::event_loop::{ControlFlow, EventLoop};

/// Initialize logging appropriately for the current platform.
fn init_logging() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Respect `RUST_LOG`, but default to something useful for a graphics app.
        env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("slmsttaa=info,wgpu_core=warn"),
        )
        .init();
    }

    #[cfg(target_arch = "wasm32")]
    {
        // Route panics and logs to the browser console.
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
    }
}

/// Open a window and run the engine's demo scene until the user quits.
///
/// This is a convenience entry point intended for examples and the bundled
/// binary. Embedders who want more control can drive [`App`] against their own
/// [`winit`] event loop instead.
///
/// The renderer is delivered to the loop as a user event, so the event loop is
/// parameterized over [`Renderer`].
pub fn run() -> Result<(), winit::error::EventLoopError> {
    init_logging();

    // `with_user_event` lets the async-built renderer be handed back into the
    // loop, which is required on the web where we can't block on GPU init.
    let event_loop = EventLoop::<Renderer>::with_user_event().build()?;
    // `Poll` keeps us rendering continuously, which is what you want for an
    // engine. Switch to `Wait` for a GUI-style, redraw-on-demand app.
    event_loop.set_control_flow(ControlFlow::Poll);

    let app = App::new(event_loop.create_proxy());

    // Native blocks here until the loop exits. The web can't block the main
    // thread, so winit drives the loop off the browser's animation frames via
    // `spawn_app` — calling `run_app` on the web throws a control-flow
    // exception and never returns normally.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        event_loop.run_app(&mut app)
    }

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
        Ok(())
    }
}

/// WASM entry point. Call this from JavaScript after loading the module.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() {
    // Errors here can't be propagated to JS meaningfully; log and move on.
    if let Err(err) = run() {
        log::error!("slmsttaa failed to start: {err}");
    }
}
