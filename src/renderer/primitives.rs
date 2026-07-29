//! Primitive geometry builders: the alternative to an asset pipeline.
//!
//! Every shape here is authored in **object space**, centered on the origin, with
//! correct outward normals and counter-clockwise winding. A consumer composes them
//! under [`Transform`](crate::Transform)s rather than importing a model — which
//! costs zero dependencies, no file I/O, and no wasm asset-fetching story. See
//! `ROADMAP.md`, which takes that trade deliberately.
//!
//! **Why these live in the engine at all.** A mesh builder is not GPU plumbing,
//! so it is worth stating: this is entirely *content-free* geometry construction.
//! It encodes no consumer semantics — compare a `Terrain` builder, which would.
//! Every consumer needs a box; none of them need *our* box.
//!
//! **They emit white vertices.** Color is a per-instance
//! [`Material`](crate::Material) tint now, so baking a color into shared geometry
//! would be exactly the mistake instancing exists to avoid: every placement of one
//! mesh would be stuck with it. A demo that wants per-vertex color (a height
//! palette, a rainbow cube) is building something these builders don't describe,
//! and should hand-write it through [`Mesh::new`].
//!
//! **Lighting is what forces the vertex counts.** A lit cuboid cannot share its
//! eight corners: a corner touches three faces pointing three different ways, and
//! a vertex carries exactly one normal. So a box is 24 vertices, not 8. Writing
//! that out by hand for the fifth shape is the roadblock this module removes.

use crate::renderer::{Mesh, Vertex};

/// One vertex of a primitive: white, with the normal the caller worked out.
fn vertex(position: [f32; 3], normal: [f32; 3]) -> Vertex {
    Vertex {
        position,
        normal,
        color: Vertex::WHITE,
    }
}

/// The eight corners of a unit cube, in the order the face table indexes them.
const CUBE_CORNERS: [[f32; 3]; 8] = [
    [-0.5, -0.5, -0.5],
    [0.5, -0.5, -0.5],
    [0.5, 0.5, -0.5],
    [-0.5, 0.5, -0.5],
    [-0.5, -0.5, 0.5],
    [0.5, -0.5, 0.5],
    [0.5, 0.5, 0.5],
    [-0.5, 0.5, 0.5],
];

/// Each face as four corner indices wound counter-clockwise seen from outside,
/// plus the direction it points.
#[rustfmt::skip]
const CUBE_FACES: [([usize; 4], [f32; 3]); 6] = [
    ([4, 5, 6, 7], [ 0.0,  0.0,  1.0]), // front  (+z)
    ([0, 3, 2, 1], [ 0.0,  0.0, -1.0]), // back   (-z)
    ([1, 2, 6, 5], [ 1.0,  0.0,  0.0]), // right  (+x)
    ([0, 4, 7, 3], [-1.0,  0.0,  0.0]), // left   (-x)
    ([3, 7, 6, 2], [ 0.0,  1.0,  0.0]), // top    (+y)
    ([0, 1, 5, 4], [ 0.0, -1.0,  0.0]), // bottom (-y)
];

/// Push the two counter-clockwise triangles of a quad whose four vertices were
/// just appended starting at `base`.
fn quad(indices: &mut Vec<u32>, base: u32) {
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

impl Mesh {
    /// A flat rectangle on the XZ plane, facing up, centered on the origin.
    ///
    /// `size` is `[width along X, depth along Z]`. Two triangles — a ground plane
    /// or a wall, depending where you put it.
    pub fn plane(size: [f32; 2]) -> Self {
        let (hx, hz) = (size[0] * 0.5, size[1] * 0.5);
        // Counter-clockwise seen from above, so back-face culling keeps the top.
        let vertices = [
            [-hx, 0.0, -hz],
            [-hx, 0.0, hz],
            [hx, 0.0, hz],
            [hx, 0.0, -hz],
        ]
        .iter()
        .map(|&p| vertex(p, Vertex::UP))
        .collect();
        Self::new(vertices, vec![0, 1, 2, 0, 2, 3])
    }

    /// A box centered on the origin, `size` units across on each axis.
    ///
    /// 24 vertices, not 8 — see the module docs. Pass `[s; 3]` for a cube.
    pub fn cuboid(size: [f32; 3]) -> Self {
        let mut vertices = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        for (corners, normal) in CUBE_FACES {
            let base = vertices.len() as u32;
            for corner in corners {
                let c = CUBE_CORNERS[corner];
                vertices.push(vertex(
                    [c[0] * size[0], c[1] * size[1], c[2] * size[2]],
                    normal,
                ));
            }
            quad(&mut indices, base);
        }
        Self::new(vertices, indices)
    }

    /// A UV sphere centered on the origin.
    ///
    /// `segments` divides it around the Y axis (longitude) and `rings` from pole
    /// to pole (latitude); both are clamped to a sane minimum. A sphere's normal
    /// is just its outward direction, so this is the one primitive whose normals
    /// need no thought.
    pub fn sphere(radius: f32, segments: u32, rings: u32) -> Self {
        let segments = segments.max(3);
        let rings = rings.max(2);

        // Latitudes run north pole (0) to south pole (rings).
        let latitudes = (0..=rings).map(|ring| {
            let theta = std::f32::consts::PI * ring as f32 / rings as f32;
            (theta.cos(), theta.sin())
        });
        // No offsets: every ring sits on the sphere itself.
        Self::lathe(radius, segments, &latitudes.collect::<Vec<_>>(), &[])
    }

    /// A capsule centered on the origin: a cylinder of `length` along the Y axis
    /// capped with a hemisphere at each end.
    ///
    /// Total height is `length + 2.0 * radius`. `rings` counts latitudes **per
    /// hemisphere**. The limb shape — which is why it is here rather than a bare
    /// cylinder.
    pub fn capsule(radius: f32, length: f32, segments: u32, rings: u32) -> Self {
        let segments = segments.max(3);
        let rings = rings.max(1);
        let half = length.max(0.0) * 0.5;

        // The two hemispheres' latitudes, with the equator appearing twice — once
        // at the top cap's rim and once at the bottom's. The quad spanning that
        // duplicated pair *is* the cylinder wall, and it falls out of the same
        // triangulation for free.
        let mut latitudes = Vec::with_capacity(2 * (rings as usize + 1));
        let mut offsets = Vec::with_capacity(2 * (rings as usize + 1));
        for ring in 0..=rings {
            let theta = std::f32::consts::FRAC_PI_2 * ring as f32 / rings as f32;
            latitudes.push((theta.cos(), theta.sin()));
            offsets.push(half);
        }
        for ring in 0..=rings {
            let theta = std::f32::consts::FRAC_PI_2 * (1.0 + ring as f32 / rings as f32);
            latitudes.push((theta.cos(), theta.sin()));
            offsets.push(-half);
        }
        Self::lathe(radius, segments, &latitudes, &offsets)
    }

    /// Build a surface of revolution from a list of latitudes.
    ///
    /// Each latitude is `(cos θ, sin θ)` — the unit normal's Y component and its
    /// horizontal radius — and `offsets` shifts that ring along Y *without*
    /// tilting its normal, which is what lets one routine emit both a sphere
    /// (every offset zero) and a capsule (the two hemispheres pushed apart).
    ///
    /// Shared by [`Mesh::sphere`] and [`Mesh::capsule`] because the winding and
    /// the pole degeneracies are the fiddly part, and having them in one place is
    /// the difference between one correct implementation and two nearly-correct
    /// ones.
    fn lathe(radius: f32, segments: u32, latitudes: &[(f32, f32)], offsets: &[f32]) -> Self {
        let rows = latitudes.len();
        let cols = segments as usize + 1; // the seam column is duplicated
        let mut vertices = Vec::with_capacity(rows * cols);

        // A sphere passes no offsets at all; a capsule one per row.
        let offset_of = |row: usize| *offsets.get(row).unwrap_or(&0.0);

        for (row, &(cos_theta, sin_theta)) in latitudes.iter().enumerate() {
            let offset = offset_of(row);
            for col in 0..cols {
                let phi = std::f32::consts::TAU * col as f32 / segments as f32;
                let (sin_phi, cos_phi) = phi.sin_cos();
                let normal = [sin_theta * sin_phi, cos_theta, sin_theta * cos_phi];
                vertices.push(vertex(
                    [
                        normal[0] * radius,
                        normal[1] * radius + offset,
                        normal[2] * radius,
                    ],
                    normal,
                ));
            }
        }

        let index = |row: usize, col: usize| (row * cols + col) as u32;
        let mut indices = Vec::with_capacity(rows * cols * 6);
        for row in 0..rows - 1 {
            // Two consecutive rows can describe the *same* circle: a capsule of
            // zero length puts both copies of the equator at y = 0, and the band
            // between them has no area. Emitting it would fill the index buffer
            // with slivers that rasterize to nothing and draw spurious wireframe
            // edges, so skip the pair outright.
            let (cos_a, sin_a) = latitudes[row];
            let (cos_b, sin_b) = latitudes[row + 1];
            let y_a = cos_a * radius + offset_of(row);
            let y_b = cos_b * radius + offset_of(row + 1);
            if (y_a - y_b).abs() < 1e-9 && (sin_a - sin_b).abs() < 1e-9 {
                continue;
            }

            for col in 0..segments as usize {
                let (a, b) = (index(row, col), index(row, col + 1));
                let (c, d) = (index(row + 1, col), index(row + 1, col + 1));
                // At a pole every vertex of that row sits on the same point, so one
                // of the two triangles collapses to a line. Skipping it keeps the
                // index buffer honest — and keeps the wireframe edge list from
                // sprouting spokes that aren't there.
                //
                // Which one collapses is the opposite of the intuitive guess: at
                // the *north* pole `a` and `b` coincide, so it is the second
                // triangle that dies, and vice versa at the south.
                let at_north = row == 0 && latitudes[0].1.abs() < 1e-6;
                let at_south = row == rows - 2 && latitudes[rows - 1].1.abs() < 1e-6;
                if !at_south {
                    indices.extend_from_slice(&[a, c, d]);
                }
                if !at_north {
                    indices.extend_from_slice(&[a, d, b]);
                }
            }
        }
        Self::new(vertices, indices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The engine's first tests, and worth saying why they're allowed to exist
    // here when `CLAUDE.md` says the engine half is verified by looking at it:
    // these builders are **pure CPU geometry**. No GPU, no surface, no event
    // loop. The winding rule they have to satisfy is also exactly the kind of
    // thing that is invisible until a face vanishes on screen — the pole
    // degeneracy was inverted on the first attempt, and a zero-length capsule
    // shipped a whole band of degenerate triangles. Both were caught here.

    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// Every normal should be unit length — the shader renormalizes after
    /// interpolation, but a mesh that ships un-normalized normals is wrong at the
    /// vertices too.
    fn assert_unit_normals(mesh: &Mesh) {
        for v in &mesh.vertices {
            let len = dot(v.normal, v.normal).sqrt();
            assert!(
                (len - 1.0).abs() < 1e-3,
                "normal {:?} has length {len}",
                v.normal
            );
        }
    }

    /// For a **convex shape centered on the origin**, a correctly wound triangle's
    /// geometric normal points away from the origin. That single check catches
    /// both inverted winding and the degenerate-triangle mistakes at the poles.
    fn assert_outward_winding(mesh: &Mesh) {
        assert_eq!(mesh.indices.len() % 3, 0, "indices are not whole triangles");
        for tri in mesh.indices.chunks_exact(3) {
            let p: [[f32; 3]; 3] = std::array::from_fn(|k| mesh.vertices[tri[k] as usize].position);
            let face = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            let area = dot(face, face).sqrt();
            assert!(area > 1e-9, "degenerate triangle at {p:?}");
            let centroid = [
                (p[0][0] + p[1][0] + p[2][0]) / 3.0,
                (p[0][1] + p[1][1] + p[2][1]) / 3.0,
                (p[0][2] + p[1][2] + p[2][2]) / 3.0,
            ];
            assert!(
                dot(face, centroid) > 0.0,
                "triangle {p:?} is wound inward (facing the origin)"
            );
        }
    }

    #[test]
    fn cuboid_has_split_corners_and_outward_faces() {
        let mesh = Mesh::cuboid([2.0, 1.0, 3.0]);
        // 6 faces x 4 corners: a lit box cannot share its eight corners.
        assert_eq!(mesh.vertices.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
        assert_unit_normals(&mesh);
        assert_outward_winding(&mesh);
        // The requested size is the full extent, not a half-extent.
        let max_x = mesh
            .vertices
            .iter()
            .fold(f32::MIN, |m, v| m.max(v.position[0]));
        assert!((max_x - 1.0).abs() < 1e-6);
    }

    #[test]
    fn plane_faces_up() {
        let mesh = Mesh::plane([4.0, 2.0]);
        assert_eq!(mesh.vertices.len(), 4);
        assert_unit_normals(&mesh);
        for v in &mesh.vertices {
            assert_eq!(v.normal, Vertex::UP);
            assert_eq!(v.position[1], 0.0);
        }
        // Wound counter-clockwise seen from above: the geometric normal is +Y.
        let p: Vec<_> = mesh.indices[..3]
            .iter()
            .map(|&i| mesh.vertices[i as usize].position)
            .collect();
        assert!(cross(sub(p[1], p[0]), sub(p[2], p[0]))[1] > 0.0);
    }

    #[test]
    fn sphere_is_closed_and_outward() {
        let mesh = Mesh::sphere(2.0, 16, 8);
        assert_unit_normals(&mesh);
        assert_outward_winding(&mesh);
        // Every point sits on the sphere, and the normal is the outward direction.
        for v in &mesh.vertices {
            let r = dot(v.position, v.position).sqrt();
            assert!((r - 2.0).abs() < 1e-4, "radius {r}");
            assert!(dot(v.normal, v.position) > 0.0);
        }
    }

    #[test]
    fn capsule_is_a_cylinder_between_two_caps() {
        let (radius, length) = (0.5f32, 3.0f32);
        let mesh = Mesh::capsule(radius, length, 12, 4);
        assert_unit_normals(&mesh);
        assert_outward_winding(&mesh);

        // Total height is the barrel plus both hemispheres.
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for v in &mesh.vertices {
            lo = lo.min(v.position[1]);
            hi = hi.max(v.position[1]);
        }
        assert!((hi - (length / 2.0 + radius)).abs() < 1e-5, "top at {hi}");
        assert!(
            (lo + (length / 2.0 + radius)).abs() < 1e-5,
            "bottom at {lo}"
        );

        // Nothing bulges past the barrel radius.
        for v in &mesh.vertices {
            let r = (v.position[0].powi(2) + v.position[2].powi(2)).sqrt();
            assert!(r <= radius + 1e-5, "radius {r} exceeds {radius}");
        }
    }

    #[test]
    fn a_zero_length_capsule_is_a_sphere() {
        // The degenerate case is worth pinning: it is what a limb collapses to
        // when a demo drives `length` from a slider that reaches zero.
        let mesh = Mesh::capsule(1.0, 0.0, 12, 4);
        assert_unit_normals(&mesh);
        assert_outward_winding(&mesh);
        for v in &mesh.vertices {
            let r = dot(v.position, v.position).sqrt();
            assert!((r - 1.0).abs() < 1e-4, "radius {r}");
        }
    }

    #[test]
    fn degenerate_segment_counts_are_clamped_not_panicked() {
        for mesh in [Mesh::sphere(1.0, 0, 0), Mesh::capsule(1.0, 1.0, 0, 0)] {
            assert!(!mesh.indices.is_empty());
            assert_outward_winding(&mesh);
        }
    }
}
