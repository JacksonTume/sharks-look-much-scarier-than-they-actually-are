//! # SLMSTTAA — Sharks Look Much Scarier Than They Actually Are
//!
//! A small, approachable Rust 3D rendering engine built on [`wgpu`] and
//! [`winit`]. Despite the intimidating name, the internals are friendlier than
//! they look.
//!
//! The crate is split into a few focused modules:
//!
//! - [`application`] — the [`Application`] trait a consumer implements.
//! - [`app`]     — windowing + event loop glue (winit `ApplicationHandler`).
//! - [`renderer`] — the wgpu device/surface/pipeline plumbing.
//! - [`camera`]  — a minimal perspective camera producing a view-projection matrix.
//!
//! To put something on screen, implement [`Application`] and pass it to [`run`],
//! which opens a window and drives your consumer until it is closed. See the
//! `triangle` example for the smallest possible consumer.

pub mod app;
pub mod application;
pub mod camera;
pub mod renderer;

pub use app::App;
pub use application::Application;
pub use camera::Camera;
pub use renderer::{Renderer, Vertex};

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

/// Open a window and drive `app` until the user quits.
///
/// This is the engine's entry point: it owns the window, GPU, and event loop and
/// calls into your [`Application`]'s `init`/`update` hooks. Embedders who want
/// more control can drive [`App`] against their own [`winit`] event loop instead.
///
/// The renderer is delivered to the loop as a user event, so the event loop is
/// parameterized over [`Renderer`].
pub fn run<A: Application + 'static>(app: A) -> Result<(), winit::error::EventLoopError> {
    init_logging();

    // `with_user_event` lets the async-built renderer be handed back into the
    // loop, which is required on the web where we can't block on GPU init.
    let event_loop = EventLoop::<Renderer>::with_user_event().build()?;
    // `Poll` keeps us rendering continuously, which is what you want for an
    // engine. Switch to `Wait` for a GUI-style, redraw-on-demand app.
    event_loop.set_control_flow(ControlFlow::Poll);

    // The engine only ever sees `dyn Application`; it cannot know the consumer.
    let app = App::new(event_loop.create_proxy(), Box::new(app));

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
