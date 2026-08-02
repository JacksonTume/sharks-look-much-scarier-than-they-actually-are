//! A small render graph: the passes a frame is made of, and the textures they
//! hand to each other.
//!
//! # Why this exists, and what it deliberately is not
//!
//! Through Slice 15 a frame was two passes wired by hand in `render()` — 3D, then
//! overlay — and that was the right amount of machinery for two passes with no
//! data flowing between them. Slice 16 broke that: water has to *sample* the
//! opaque scene to refract and reflect it, so a pass now depends on another
//! pass's output, and the frame went to four. Two things stop scaling at exactly
//! that point:
//!
//! - **Ordering became a correctness property rather than a reading order.** With
//!   no dependencies, passes ran in the order they were written because that's
//!   the order they appeared. With one, "the blended pass must run after the
//!   opaque pass because it reads what the opaque pass wrote" is a fact about the
//!   frame that was previously enforced by nothing at all.
//! - **Every offscreen target must track the surface size.** `ARCHITECTURE.md`
//!   already records the depth buffer version of this as a gotcha learned the
//!   hard way. Slice 16 adds two more textures with the same requirement, and
//!   three hand-resized attachments is where someone eventually forgets one.
//!
//! So resources are *declared* with a format, passes are *declared* with what
//! they read and write, and this module resolves the order, allocates the
//! textures, and re-allocates them on resize. That is the whole of it.
//!
//! **It is not a general-purpose render graph and does not try to be.** There is
//! no transient-memory aliasing, no automatic barrier insertion (wgpu already
//! does that), no async-compute scheduling, and — most importantly — **no way
//! for a consumer to add a pass**. The engine roadmap's stated trigger for a
//! *public* graph is "a second consumer wanting its own pass", and no consumer
//! wants one yet; what pulled this into existence is the engine's own fourth
//! pass. So the whole module is `pub(crate)`, [`PassKind`] is a closed enum, and
//! widening either waits for the demo that asks. What it buys today is that
//! adding a pass means declaring what it touches, and being wrong about that is
//! a panic at build time rather than a subtly wrong picture.
//!
//! # The one bug it is built to make impossible
//!
//! A texture cannot be a render target and a sampled input in the same pass —
//! reading the surface you are drawing to is undefined, and it is the obvious
//! mistake when the water pass wants "the scene behind the water" and the scene
//! is right there. [`RenderGraph::build`] rejects it outright. That is why the
//! opaque pass renders to an offscreen texture and a separate composite pass
//! copies it to the swapchain, rather than the blended pass simply drawing over
//! what is already on screen.

use std::collections::HashSet;

use super::DEPTH_FORMAT;

/// What a graph-owned texture holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceFormat {
    /// The surface's colour format, so a composite is a straight copy and the
    /// pipelines targeting it need no format permutation.
    Color,
    /// [`DEPTH_FORMAT`].
    Depth,
}

/// A handle to a texture declared on the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ResourceId(usize);

/// The swapchain image — the one resource the graph does not own.
///
/// Its view arrives per frame from `get_current_texture`, so it is declared
/// first and always exists; [`RenderGraph::record`] takes it as an argument.
pub(crate) const SWAPCHAIN: ResourceId = ResourceId(0);

/// A texture the graph owns, sized to the surface.
#[derive(Debug)]
struct Resource {
    label: &'static str,
    format: ResourceFormat,
    /// Whether any pass reads it, which decides `TEXTURE_BINDING` usage. Derived
    /// from the declarations rather than stated, so it cannot disagree with them.
    sampled: bool,
    /// `None` for the swapchain, and until [`RenderGraph::allocate`] runs.
    view: Option<wgpu::TextureView>,
}

/// Which of the engine's passes a [`Pass`] is.
///
/// A closed enum rather than a boxed closure, and that is a deliberate trade. A
/// closure would let a pass carry its own recording logic, which is what a public
/// graph needs — but it would also have to borrow the `Renderer` that owns the
/// graph, and the resulting lifetime dance buys flexibility nothing is asking
/// for. `Renderer::render` matches on this instead: the graph decides *what runs
/// when and against which views*, and the renderer knows how to record each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PassKind {
    /// Fullscreen analytic sky, so the scene has a horizon behind it rather than
    /// a flat clear colour — and so a reflection ray that hits nothing has
    /// somewhere to land.
    Sky,
    /// The opaque half of the draw-list, plus depth.
    Opaque,
    /// Copies the offscreen scene colour to the swapchain. Exists only because
    /// the blended pass cannot sample the target it is drawing to.
    Composite,
    /// The transparent half of the draw-list, sampling the opaque scene colour
    /// and depth for refraction and screen-space reflection.
    Blended,
    /// The 2D UI overlay.
    Overlay,
}

/// How a pass treats an attachment it writes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Load {
    /// Start from this colour.
    Clear(wgpu::Color),
    /// Start from the far plane.
    ClearDepth,
    /// Keep what is already there.
    Keep,
}

/// One pass, and everything it touches.
#[derive(Debug)]
pub(crate) struct Pass {
    pub(crate) label: &'static str,
    pub(crate) kind: PassKind,
    /// Textures sampled by this pass's shaders. These are what create the
    /// ordering edges.
    reads: Vec<ResourceId>,
    /// Colour attachment, if any.
    color: Option<(ResourceId, Load)>,
    /// Depth attachment, if any, and whether the pass writes to it. A pass that
    /// only tests depth declares `false` and gets a read-only attachment, which
    /// is what makes it legal to sample the same depth texture it is testing
    /// against.
    depth: Option<(ResourceId, Load, bool)>,
}

impl Pass {
    /// Declare a pass. Chain [`reads`](Self::reads), [`writes`](Self::writes) and
    /// [`depth`](Self::depth) onto it.
    pub(crate) fn new(label: &'static str, kind: PassKind) -> Self {
        Self {
            label,
            kind,
            reads: Vec::new(),
            color: None,
            depth: None,
        }
    }

    /// Textures this pass samples.
    pub(crate) fn reads(mut self, ids: &[ResourceId]) -> Self {
        self.reads.extend_from_slice(ids);
        self
    }

    /// The colour attachment this pass renders to.
    pub(crate) fn writes(mut self, id: ResourceId, load: Load) -> Self {
        self.color = Some((id, load));
        self
    }

    /// The depth attachment, and whether this pass writes it or only tests it.
    pub(crate) fn depth(mut self, id: ResourceId, load: Load, write: bool) -> Self {
        self.depth = Some((id, load, write));
        self
    }
}

/// The frame, as a set of declared passes and the textures between them.
#[derive(Debug)]
pub(crate) struct RenderGraph {
    /// The surface's format, which every [`ResourceFormat::Color`] resource
    /// shares so a composite is a straight copy.
    color_format: wgpu::TextureFormat,
    resources: Vec<Resource>,
    passes: Vec<Pass>,
    /// Indices into `passes`, in dependency order. Resolved once by
    /// [`RenderGraph::build`], because the frame's shape does not change between
    /// frames — only its contents do.
    order: Vec<usize>,
}

/// Everything one pass needs to begin, resolved from the declarations.
pub(crate) struct PassTargets<'a> {
    pub(crate) label: &'static str,
    pub(crate) kind: PassKind,
    pub(crate) color: Option<(&'a wgpu::TextureView, Load)>,
    pub(crate) depth: Option<(&'a wgpu::TextureView, Load, bool)>,
}

impl RenderGraph {
    /// Declare the frame.
    ///
    /// `passes` may be given in any order; [`build`](Self::build) sorts them.
    /// They are written in reading order anyway, because a frame that reads
    /// top-to-bottom is easier to follow — the point is that the order is no
    /// longer *load-bearing*.
    pub(crate) fn new(color_format: wgpu::TextureFormat) -> Self {
        Self {
            color_format,
            // The swapchain is declared first so `SWAPCHAIN` is a constant.
            // `sampled` is false and must stay false: sampling the image you are
            // presenting is exactly the mistake this module exists to reject.
            resources: vec![Resource {
                label: "swapchain",
                format: ResourceFormat::Color,
                sampled: false,
                view: None,
            }],
            passes: Vec::new(),
            order: Vec::new(),
        }
    }

    /// Declare a graph-owned texture, sized to the surface.
    pub(crate) fn resource(&mut self, label: &'static str, format: ResourceFormat) -> ResourceId {
        self.resources.push(Resource {
            label,
            format,
            sampled: false,
            view: None,
        });
        ResourceId(self.resources.len() - 1)
    }

    /// Add a declared pass.
    pub(crate) fn pass(&mut self, pass: Pass) {
        self.passes.push(pass);
    }

    /// Resolve pass order, mark which resources are sampled, and allocate.
    ///
    /// # Panics
    ///
    /// On a frame that cannot be scheduled, which is always a bug in the
    /// declarations rather than in anything a consumer did:
    ///
    /// - a pass that reads a texture it also writes (see the module docs — this
    ///   is the mistake worth a panic),
    /// - a cycle between passes,
    /// - anything sampling the swapchain image.
    ///
    /// Several passes writing the *same* colour target is expressly fine and is
    /// what the frame does — composite, blended and overlay all end up on the
    /// swapchain. [`Load::Keep`] is how the later ones say so, and it is also
    /// what orders them.
    pub(crate) fn build(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        for pass in &self.passes {
            let written: Vec<ResourceId> = pass
                .color
                .iter()
                .map(|(id, _)| *id)
                .chain(pass.depth.iter().filter(|(_, _, w)| *w).map(|(id, ..)| *id))
                .collect();
            for id in &written {
                assert!(
                    !pass.reads.contains(id),
                    "pass {:?} both reads and writes {:?} — a texture cannot be an \
                     attachment and a sampled input at once",
                    pass.label,
                    self.resources[id.0].label,
                );
            }
        }

        // A resource needs TEXTURE_BINDING exactly when something reads it.
        let read_by_someone: HashSet<ResourceId> = self
            .passes
            .iter()
            .flat_map(|p| p.reads.iter().copied())
            .collect();
        for (i, res) in self.resources.iter_mut().enumerate() {
            res.sampled = read_by_someone.contains(&ResourceId(i));
        }
        assert!(
            !self.resources[SWAPCHAIN.0].sampled,
            "the swapchain image cannot be sampled",
        );

        self.order = self.resolve_order();
        self.allocate(device, width, height);
    }

    /// Topologically sort passes: a pass that reads a resource runs after every
    /// pass that writes it.
    ///
    /// Ties are broken by declaration order, so a frame whose passes have no
    /// dependencies at all still runs in the order it was written — which keeps
    /// the overlay last without the overlay having to invent a false dependency
    /// on the scene to say so.
    fn resolve_order(&self) -> Vec<usize> {
        let writers = |id: ResourceId| -> Vec<usize> {
            self.passes
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.color.map(|(c, _)| c == id).unwrap_or(false)
                        || p.depth.map(|(d, _, w)| w && d == id).unwrap_or(false)
                })
                .map(|(i, _)| i)
                .collect()
        };

        let n = self.passes.len();
        // `deps[i]` = passes that must precede `i`.
        let mut deps: Vec<HashSet<usize>> = vec![HashSet::new(); n];
        for (i, pass) in self.passes.iter().enumerate() {
            for id in &pass.reads {
                for w in writers(*id) {
                    if w != i {
                        deps[i].insert(w);
                    }
                }
            }
            // A pass that *keeps* an attachment's contents depends on whoever
            // wrote it **before** — and "before" here means declaration order,
            // not "every other writer".
            //
            // That distinction is load-bearing and was got wrong first time. A
            // `reads` edge is a data dependency: this pass needs a finished
            // texture, so it must follow every pass that produces one. `Keep` is
            // not that. It says "carry on from whatever is already in this
            // target", which is a statement about *accumulation*, and three
            // passes accumulating onto the swapchain — composite, blended,
            // overlay — is the frame working as intended. Treating each of them
            // as depending on all the others makes them mutually dependent, and
            // the cycle check below correctly refused to schedule it. Declaration
            // order is the only thing that can say which layer goes down first,
            // so that is what orders them.
            let kept = pass
                .color
                .iter()
                .filter(|(_, l)| matches!(l, Load::Keep))
                .map(|(id, _)| *id)
                .chain(
                    pass.depth
                        .iter()
                        .filter(|(_, l, _)| matches!(l, Load::Keep))
                        .map(|(id, ..)| *id),
                );
            for id in kept {
                for w in writers(id) {
                    if w < i {
                        deps[i].insert(w);
                    }
                }
            }
        }

        let mut order = Vec::with_capacity(n);
        let mut done = vec![false; n];
        while order.len() < n {
            // Lowest-numbered pass whose dependencies are all satisfied, which is
            // what makes the tie-break declaration order.
            let next = (0..n).find(|&i| !done[i] && deps[i].iter().all(|&d| done[d]));
            let Some(next) = next else {
                let stuck: Vec<_> = (0..n)
                    .filter(|&i| !done[i])
                    .map(|i| self.passes[i].label)
                    .collect();
                panic!("render graph has a cycle among passes {stuck:?}");
            };
            done[next] = true;
            order.push(next);
        }
        order
    }

    /// (Re)create every graph-owned texture at the given size.
    ///
    /// Called by [`build`](Self::build) and again by [`resize`](Self::resize).
    /// Centralizing it is half the reason this module exists: the depth
    /// attachment used to be resized by hand next to the surface reconfigure, and
    /// Slice 16 would have added two more places to forget.
    fn allocate(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        // A zero-sized surface is a minimized window; wgpu rejects the texture.
        let width = width.max(1);
        let height = height.max(1);
        for res in &mut self.resources[1..] {
            let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
            if res.sampled {
                usage |= wgpu::TextureUsages::TEXTURE_BINDING;
            }
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(res.label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                // Must match the pipelines' 1-sample `MultisampleState`.
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: match res.format {
                    ResourceFormat::Color => self.color_format,
                    ResourceFormat::Depth => DEPTH_FORMAT,
                },
                usage,
                view_formats: &[],
            });
            res.view = Some(texture.create_view(&wgpu::TextureViewDescriptor::default()));
        }
    }

    /// Re-allocate every graph-owned texture for a new surface size.
    pub(crate) fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.allocate(device, width, height);
    }

    /// The view for a resource, for building a bind group that samples it.
    ///
    /// # Panics
    ///
    /// If asked for the swapchain, which has no persistent view.
    pub(crate) fn view(&self, id: ResourceId) -> &wgpu::TextureView {
        self.resources[id.0]
            .view
            .as_ref()
            .expect("resource has no view; the swapchain's arrives per frame")
    }

    /// The frame's passes in resolved order, with their attachment views.
    ///
    /// `swapchain` is this frame's surface view, substituted wherever
    /// [`SWAPCHAIN`] was declared.
    pub(crate) fn record<'a>(&'a self, swapchain: &'a wgpu::TextureView) -> Vec<PassTargets<'a>> {
        let view = |id: ResourceId| -> &wgpu::TextureView {
            if id == SWAPCHAIN {
                swapchain
            } else {
                self.view(id)
            }
        };
        self.order
            .iter()
            .map(|&i| {
                let pass = &self.passes[i];
                PassTargets {
                    label: pass.label,
                    kind: pass.kind,
                    color: pass.color.map(|(id, load)| (view(id), load)),
                    depth: pass.depth.map(|(id, load, write)| (view(id), load, write)),
                }
            })
            .collect()
    }
}
