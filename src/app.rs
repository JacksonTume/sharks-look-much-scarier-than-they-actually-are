//! Windowing and event-loop glue.
//!
//! [`App`] implements winit's [`ApplicationHandler`], owning the window and the
//! [`Renderer`]. GPU initialization is asynchronous, so the renderer is created
//! off to the side and delivered back into the loop as a user event — this keeps
//! the exact same code path working on native (where we just block) and on the
//! web (where blocking isn't allowed).

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::application::Application;
use crate::renderer::Renderer;

/// Top-level application state driven by the winit event loop.
pub struct App {
    /// Used to hand a freshly-built [`Renderer`] back into the event loop.
    /// Only the web path consumes this; native init blocks instead.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    proxy: EventLoopProxy<Renderer>,
    /// The consumer. The engine only ever sees it as `dyn Application`.
    application: Box<dyn Application>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// Guards against kicking off renderer creation more than once on the web.
    renderer_pending: bool,
}

impl App {
    /// Create the application. `proxy` is obtained from the event loop and is
    /// how async GPU init reports completion. `application` is the consumer the
    /// engine drives.
    pub fn new(proxy: EventLoopProxy<Renderer>, application: Box<dyn Application>) -> Self {
        Self {
            proxy,
            application,
            window: None,
            renderer: None,
            renderer_pending: false,
        }
    }

    /// Begin (or, on native, immediately complete) renderer initialization.
    fn init_renderer(&mut self, window: Arc<Window>) {
        if self.renderer.is_some() || self.renderer_pending {
            return;
        }
        self.renderer_pending = true;

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native: just block on the GPU init and hand it straight back.
            let renderer = pollster::block_on(Renderer::new(window));
            self.on_renderer_ready(renderer);
        }

        #[cfg(target_arch = "wasm32")]
        {
            // Web: spawn the async init and send the renderer back via the proxy
            // once the GPU is ready.
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let renderer = Renderer::new(window).await;
                // The loop may be gone if the page is tearing down; ignore that.
                let _ = proxy.send_event(renderer);
            });
        }
    }

    /// Funnel both platforms (native blocks, web delivers via `user_event`)
    /// through one place once the renderer exists: resync the surface, run the
    /// consumer's one-time `init`, then store it.
    fn on_renderer_ready(&mut self, mut renderer: Renderer) {
        self.renderer_pending = false;
        // The window has likely been laid out by now; resync the surface to its
        // real size in case the `Resized` event fired before the renderer
        // existed (very common on the web, where init is async).
        if let Some(window) = &self.window {
            renderer.resize(window.inner_size());
        }
        // `renderer` is still an owned local here, so this borrows it
        // independently of `self.application`.
        self.application.init(&mut renderer);
        self.renderer = Some(renderer);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler<Renderer> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes()
            .with_title("SLMSTTAA")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                log::error!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        // On the web, mount the winit canvas into the document body and size it
        // to the browser window. winit does NOT derive the canvas backing-buffer
        // size from CSS, so without this the surface is created at 1x1 and the
        // single pixel is just stretched across the page.
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowExtWebSys;

            let web_window = web_sys::window().expect("no global window");

            web_window
                .document()
                .and_then(|doc| doc.body())
                .and_then(|body| {
                    let canvas = web_sys::Element::from(window.canvas()?);
                    body.append_child(&canvas).ok()
                })
                .expect("couldn't append canvas to document body");

            // Request a logical size matching the viewport; winit scales it by
            // the device pixel ratio for a crisp backing buffer. The resulting
            // `Resized` event (and the resync in `user_event`) update the surface.
            let width = web_window
                .inner_width()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(1280.0);
            let height = web_window
                .inner_height()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(720.0);
            let _ = window.request_inner_size(winit::dpi::LogicalSize::new(width, height));
        }

        self.window = Some(window.clone());
        self.init_renderer(window);
    }

    /// Delivery point for the async-built renderer (used on the web).
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, renderer: Renderer) {
        self.on_renderer_ready(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key: Key::Named(NamedKey::Escape),
                        ..
                    },
                ..
            } => {
                event_loop.exit();
            }

            WindowEvent::Resized(new_size) => {
                renderer.resize(new_size);
            }

            // Forward input into the engine's per-frame snapshot. The consumer
            // reads it via `renderer.input()` and never sees these winit events.
            WindowEvent::KeyboardInput { event, .. } => {
                renderer.input_mut().on_keyboard(&event);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                renderer.input_mut().on_mouse_button(state, button);
            }
            WindowEvent::CursorMoved { position, .. } => {
                renderer.input_mut().on_cursor_moved(position);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                renderer.input_mut().on_scroll(delta);
            }

            WindowEvent::RedrawRequested => {
                // Let the consumer advance its state first, then draw.
                // `render` handles recoverable surface conditions internally.
                self.application.update(renderer);
                renderer.update();
                renderer.render();
                // The frame consumed this frame's input; clear the per-frame
                // deltas (held keys/buttons persist).
                renderer.input_mut().end_frame();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Keep animating: request another frame as soon as we're idle.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
