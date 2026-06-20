//! A gallery of demo scenes with on-screen buttons to switch between them.
//!
//! Like the other examples this is a separate crate that only sees the public
//! API, but it does two extra things worth noting:
//!
//! - **It owns several scenes** (a triangle, a quad, a spinning cube) and swaps
//!   the engine's draw-list between them at runtime via [`Renderer::set_meshes`] —
//!   no engine restart, no page reload.
//! - **On the web it builds real buttons.** The engine has no input hook yet
//!   (that's a later slice), so the demo creates DOM `<button>`s with `web-sys`
//!   and shares an `Rc<Cell<usize>>` with its `update` loop: a click sets the
//!   selected scene, and the next `update` notices and swaps geometry.
//!
//! On native there is no DOM, so the gallery instead **auto-cycles** through the
//! scenes every few seconds — the example stays meaningful on both targets
//! (principle: always keep something on screen).
//!
//! Run it:
//!   native — `cargo run --example gallery`
//!   web    — build for wasm and run `wasm-bindgen` (see `README.md`), substituting
//!            `gallery` for `triangle`.

use std::cell::Cell;
use std::rc::Rc;

use slmsttaa::{run, Application, Mesh, Renderer, Vertex};

/// The scenes the gallery can show, in button order.
const SCENES: [Scene; 3] = [Scene::Triangle, Scene::Quad, Scene::Cube];

/// On native, advance to the next scene every this many frames (~3s at 60fps).
#[cfg(not(target_arch = "wasm32"))]
const NATIVE_CYCLE_FRAMES: u32 = 180;

/// One selectable scene. Each knows its button label, whether it animates, and how
/// to build its [`Mesh`] (rotated by `angle` for the ones that spin).
#[derive(Clone, Copy)]
enum Scene {
    Triangle,
    Quad,
    Cube,
}

impl Scene {
    /// Text shown on the scene's button. Only used on the web (native auto-cycles).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn label(self) -> &'static str {
        match self {
            Scene::Triangle => "Triangle",
            Scene::Quad => "Quad",
            Scene::Cube => "Cube",
        }
    }

    /// Whether the scene animates and so must be re-uploaded every frame.
    fn spins(self) -> bool {
        matches!(self, Scene::Cube)
    }

    /// Build the scene's geometry at rotation `angle` (ignored by static scenes).
    fn build(self, angle: f32) -> Mesh {
        match self {
            Scene::Triangle => Mesh::new(
                vec![
                    Vertex {
                        position: [0.0, 0.5, 0.0],
                        color: [1.0, 0.2, 0.3],
                    },
                    Vertex {
                        position: [-0.5, -0.5, 0.0],
                        color: [0.2, 1.0, 0.4],
                    },
                    Vertex {
                        position: [0.5, -0.5, 0.0],
                        color: [0.3, 0.4, 1.0],
                    },
                ],
                vec![0, 1, 2],
            ),
            Scene::Quad => Mesh::new(
                vec![
                    Vertex {
                        position: [-0.5, -0.5, 0.0],
                        color: [1.0, 0.2, 0.3],
                    },
                    Vertex {
                        position: [0.5, -0.5, 0.0],
                        color: [0.2, 1.0, 0.4],
                    },
                    Vertex {
                        position: [0.5, 0.5, 0.0],
                        color: [0.3, 0.4, 1.0],
                    },
                    Vertex {
                        position: [-0.5, 0.5, 0.0],
                        color: [1.0, 1.0, 0.3],
                    },
                ],
                // Two triangles, wound CCW (front toward the camera).
                vec![0, 1, 2, 0, 2, 3],
            ),
            Scene::Cube => cube_mesh(angle),
        }
    }
}

/// The 8 corners of a unit cube, colored by position (matches `examples/cube.rs`).
const CUBE_CORNERS: [([f32; 3], [f32; 3]); 8] = [
    ([-0.5, -0.5, -0.5], [0.0, 0.0, 0.0]),
    ([0.5, -0.5, -0.5], [1.0, 0.0, 0.0]),
    ([0.5, 0.5, -0.5], [1.0, 1.0, 0.0]),
    ([-0.5, 0.5, -0.5], [0.0, 1.0, 0.0]),
    ([-0.5, -0.5, 0.5], [0.0, 0.0, 1.0]),
    ([0.5, -0.5, 0.5], [1.0, 0.0, 1.0]),
    ([0.5, 0.5, 0.5], [1.0, 1.0, 1.0]),
    ([-0.5, 0.5, 0.5], [0.0, 1.0, 1.0]),
];

/// 12 triangles, each wound CCW from outside so back-face culling keeps the solid.
#[rustfmt::skip]
const CUBE_INDICES: [u32; 36] = [
    4, 5, 6,  4, 6, 7, // front  (+z)
    0, 2, 1,  0, 3, 2, // back   (-z)
    1, 2, 6,  1, 6, 5, // right  (+x)
    0, 4, 7,  0, 7, 3, // left   (-x)
    3, 7, 6,  3, 6, 2, // top    (+y)
    0, 1, 5,  0, 5, 4, // bottom (-y)
];

/// Build the cube rotated by `angle` (yaw about Y, plus a gentler pitch about X).
fn cube_mesh(angle: f32) -> Mesh {
    let (sy, cy) = angle.sin_cos();
    let (sp, cp) = (angle * 0.6).sin_cos();
    let vertices = CUBE_CORNERS
        .iter()
        .map(|&(p, color)| {
            let x = p[0] * cy + p[2] * sy;
            let z = -p[0] * sy + p[2] * cy;
            let y = p[1];
            Vertex {
                position: [x, y * cp - z * sp, y * sp + z * cp],
                color,
            }
        })
        .collect();
    Mesh::new(vertices, CUBE_INDICES.to_vec())
}

/// The gallery consumer: tracks which scene is selected and re-uploads on change.
struct GalleryDemo {
    /// Selected scene index. On the web, button clicks write to this; `update`
    /// reads it. Shared via `Rc` so the DOM click closures can hold their own clone.
    selected: Rc<Cell<usize>>,
    /// The scene currently uploaded, so we only re-upload when it changes.
    current: usize,
    /// Rotation accumulator for animated scenes.
    angle: f32,
    /// Native-only frame counter driving the auto-cycle.
    #[cfg(not(target_arch = "wasm32"))]
    frames: u32,
}

impl GalleryDemo {
    fn new() -> Self {
        Self {
            selected: Rc::new(Cell::new(0)),
            current: 0,
            angle: 0.0,
            #[cfg(not(target_arch = "wasm32"))]
            frames: 0,
        }
    }
}

impl Application for GalleryDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        #[cfg(target_arch = "wasm32")]
        create_buttons(&self.selected);

        let sel = self.selected.get();
        renderer.set_meshes(&[SCENES[sel].build(0.0)]);
        self.current = sel;
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Native has no buttons, so step through the scenes automatically.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.frames += 1;
            if self.frames % NATIVE_CYCLE_FRAMES == 0 {
                let next = (self.selected.get() + 1) % SCENES.len();
                self.selected.set(next);
            }
        }

        let sel = self.selected.get();
        if sel != self.current {
            // Scene changed (button click or auto-cycle): swap geometry.
            self.current = sel;
            self.angle = 0.0;
            renderer.set_meshes(&[SCENES[sel].build(0.0)]);
        } else if SCENES[self.current].spins() {
            // Same scene, but it animates: advance and re-upload.
            self.angle += 0.01;
            renderer.set_meshes(&[SCENES[self.current].build(self.angle)]);
        }
    }
}

/// Build a small button bar (one button per scene) over the canvas. Each button's
/// click sets the shared selected-scene index that `update` polls. Web only.
#[cfg(target_arch = "wasm32")]
fn create_buttons(selected: &Rc<Cell<usize>>) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(body) = document.body() else {
        return;
    };

    let Ok(container) = document.create_element("div") else {
        return;
    };
    let _ = container.set_attribute(
        "style",
        "position:fixed;top:12px;left:12px;z-index:10;display:flex;gap:8px;",
    );

    for (i, scene) in SCENES.iter().enumerate() {
        let Ok(button) = document.create_element("button") else {
            continue;
        };
        button.set_inner_html(scene.label());
        let _ = button.set_attribute(
            "style",
            "padding:6px 12px;border:0;border-radius:6px;cursor:pointer;\
             background:#1f6feb;color:#fff;font:14px system-ui,sans-serif;",
        );

        // Each closure owns a clone of the shared selection and the scene index.
        let selected = selected.clone();
        let on_click = Closure::<dyn FnMut()>::new(move || selected.set(i));
        let _ = button.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref());
        // Hand the closure to the JS GC's keep-alive; it must outlive this call.
        on_click.forget();

        let _ = container.append_child(&button);
    }

    let _ = body.append_child(&container);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = run(GalleryDemo::new()) {
        eprintln!("gallery example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    // Errors here can't be propagated to JS meaningfully; `run` logs to the
    // browser console on its own.
    let _ = run(GalleryDemo::new());
}

// A bin example still needs a `main` to compile for the wasm target; the real
// entry point there is `start` above.
#[cfg(target_arch = "wasm32")]
fn main() {}
