//! CPU-side geometry the consumer builds and hands to the engine.

use crate::renderer::Vertex;

/// A triangle mesh: a pool of [`Vertex`]es plus the indices that connect them.
///
/// The consumer builds this CPU-side and hands it to the engine via
/// [`Renderer::set_meshes`](crate::Renderer::set_meshes); the engine owns the GPU
/// buffers. `indices` reference into `vertices` (three per triangle), so a corner
/// shared by several triangles — every interior vertex of a grid, every corner of
/// a cube — is stored once instead of duplicated per face.
///
/// Triangles are expected to be wound **counter-clockwise when viewed from the
/// front**; the engine culls back faces (see `ARCHITECTURE.md`). Getting the
/// winding right is the consumer's responsibility — it depends on the geometry,
/// not the engine.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    /// The vertex pool.
    pub vertices: Vec<Vertex>,
    /// Indices into `vertices`, three per triangle.
    pub indices: Vec<u32>,
}

impl Mesh {
    /// Build a mesh from a vertex pool and the indices that connect it.
    pub fn new(vertices: Vec<Vertex>, indices: Vec<u32>) -> Self {
        Self { vertices, indices }
    }
}
