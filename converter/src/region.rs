// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native terrain geometry and an exact-copy texture atlas. No rasterized tiles.
use crate::{
    binary::Reader,
    cache::Cache,
    terrain::{self, World},
    textures::Textures,
};
use anyhow::{bail, ensure, Context as _, Result};
use std::collections::BTreeMap;
/// Native overlay or underlay material definition.
#[derive(Clone, Debug)]
pub(crate) struct Floor {
    /// Packed 24-bit native floor color.
    pub rgb: u32,
    /// Procedural texture ID, or -1 for an untextured floor.
    pub texture: i32,
    /// Native texture repeat scale.
    pub scale: f32,
    /// Whether the overlay requests interpolation into adjacent tiles.
    pub blend: bool,
    /// Native floor blend ordering priority.
    pub priority: u8,
    /// Packed native underwater fog color.
    pub water_color: u32,
}
impl Floor {
    /// Calculate the native floor multiplier from texture brightness metadata.
    #[allow(
        clippy::indexing_slicing,
        reason = "indices follow validated native tables and allocated image dimensions"
    )]
    #[allow(
        clippy::absolute_paths,
        reason = "qualified names identify the format or IO operation directly"
    )]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        clippy::float_arithmetic,
        clippy::suboptimal_flops,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    pub(crate) fn tint(&self, texture: Option<&crate::textures::Texture>) -> [f32; 4] {
        let mut tint = [1.; 4];
        for (c, shift) in [(0, 16_i32), (1, 8_i32), (2, 0_i32)] {
            let base = ((self.rgb >> shift) & 255) as f32;
            let brightness = texture.map_or(0., |t| f32::from(t.brightness));
            let boost = texture.map_or(0., |t| f32::from(t.boost));
            tint[c] = (base * (256. - brightness) + 255. * brightness) / 256. / 255.
                * (256. + boost)
                / 256.;
        }
        tint
    }
    /// Decode an overlay or underlay floor definition.
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    fn decode(data: &[u8], overlay: bool) -> Result<Self> {
        let mut r = Reader::new(data);
        let mut f = Self {
            rgb: 0,
            texture: -1,
            scale: 128.,
            blend: false,
            priority: 8,
            water_color: 1_190_717,
        };
        loop {
            let op = r.u8()?;
            match (overlay, op) {
                (_, 0) => break,
                (_, 1) => f.rgb = r.u24()?,
                (false, 2) | (true, 3) => {
                    let id = r.u16()?;
                    f.texture = if id == 65535 { -1_i32 } else { i32::from(id) };
                }
                (false, 3) | (true, 9) => f.scale = f32::from(r.u16()?),
                (true, 2) => f.texture = i32::from(r.u8()?),
                (false, 4) | (true, 5 | 8 | 10) => {}
                (true, 7) => {
                    let _consumed = r.u24()?;
                }
                (true, 14) => {
                    let _consumed = r.u8()?;
                }
                (true, 11) => f.priority = r.u8()?,
                (true, 13) => f.water_color = r.u24()?,
                (true, 12) => f.blend = true,
                _ => bail!(
                    "Unknown {} floor opcode {op}",
                    if overlay { "overlay" } else { "underlay" }
                ),
            }
        }
        r.finish()?;
        ensure!(f.scale > 0., "Invalid texture scale");
        Ok(f)
    }
}
/// One terrain vertex with position, normal and native UV coordinates.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Vertex {
    /// Scaled Source-space position.
    pub pos: [f32; 3],
    /// Unit surface normal.
    pub normal: [f32; 3],
    /// Native independently repeating texture coordinates.
    pub uv: [f32; 2],
}
/// One native terrain triangle and its material/control bindings.
#[derive(Clone, Debug)]
pub(crate) struct Triangle {
    /// Original triangle vertices in export winding order.
    pub vertices: [Vertex; 3],
    /// Whether this surface uses the water shader.
    pub water: bool,
    /// Index into the exported material definitions.
    pub material: usize,
    /// Whether this triangle belongs to the underwater map group.
    pub underwater: bool,
    /// Surface plane or underwater plane identifier.
    pub layer: u8,
    /// Tile address in the packed interpolation control grid.
    pub blend_address: u32,
}
/// Shader parameters for one native terrain material.
#[derive(Clone, serde::Serialize)]
pub(crate) struct Surface {
    /// Packed texture rectangle: x, y, width and height.
    pub rect: [usize; 4],
    /// Native RGBA multiplier derived from floor and texture metadata.
    pub tint: [f32; 4],
    /// Procedural texture ID, or -1 for an untextured floor.
    pub texture: i32,
    /// Whether this surface uses the water shader.
    pub water: bool,
    /// Native texture repeat scale.
    pub scale: f32,
}
/// An exact-copy atlas image shared by matching source textures.
#[derive(serde::Serialize)]
pub(crate) struct AtlasImage {
    /// Cache IDs whose pixel data and dimensions are identical.
    pub source_ids: Vec<i32>,
    /// Packed texture rectangle: x, y, width and height.
    pub rect: [usize; 4],
    /// CRC32 of the unmodified native RGBA image.
    pub crc: u32,
}
/// Complete terrain geometry, native texture atlas and export metadata.
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
pub(crate) struct RegionMesh {
    /// Native terrain triangles, including underwater terrain.
    pub triangles: Vec<Triangle>,
    /// Packed atlas bytes in RGBA order.
    pub pixels: Vec<u8>,
    /// Atlas width after power-of-two cropping of unused space.
    pub width: usize,
    /// Atlas height after power-of-two cropping of unused space.
    pub height: usize,
    /// Number of source texture images requested by the region.
    pub tiles: usize,
    /// Number of distinct native images after exact deduplication.
    pub unique_tiles: usize,
    /// Minimum and maximum Source-space positions.
    pub bounds: [[f32; 3]; 2],
    /// Triangle counts for the four surface planes.
    pub plane_triangles: [usize; 4],
    /// Number of triangles decoded from underwater terrain.
    pub underwater_triangles: usize,
    /// Native terrain materials indexing the shared atlas.
    pub materials: Vec<Surface>,
    /// Native image rectangles and deduplication provenance.
    pub atlas_images: Vec<AtlasImage>,
    /// Offline RT4 interpolation plans and control image.
    pub blending: crate::blending::Blending,
    /// World-to-tile mapping parameters consumed by the atlas shader.
    pub mapping: [f32; 4],
}
/// Packed RGBA pixels, atlas dimensions, image metadata and source-ID rectangles.
#[allow(
    unmarked_unchecked_type_aliases,
    reason = "concrete alias has no generic bounds"
)]
type PackedAtlas = (
    Vec<u8>,
    [usize; 2],
    Vec<AtlasImage>,
    BTreeMap<i32, [usize; 4]>,
);
/// Deduplicate and pack native images without resizing or padding.
#[allow(
    clippy::indexing_slicing,
    reason = "indices follow validated native tables and allocated image dimensions"
)]
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn pack(textures: &Textures, control_side: Option<usize>) -> Result<PackedAtlas> {
    let mut unique: Vec<(usize, Option<&crate::textures::Texture>, Vec<i32>)> = vec![];
    for (&id, image) in &textures.images {
        ensure!(
            image.pixels.len() == image.size * image.size,
            "Invalid native texture dimensions"
        );
        if let Some(&mut (_, _, ref mut ids)) = unique.iter_mut().find(|&&mut (size, other, _)| {
            size == image.size && other.is_some_and(|texture| texture.pixels == image.pixels)
        }) {
            ids.push(id);
        } else {
            unique.push((image.size, Some(image), vec![id]));
        }
    }
    // Power-of-two native images pack on shelves, without gutters or extrusion.
    unique.sort_by_key(|&(size, _, _)| core::cmp::Reverse(size));
    if let Some(size) = control_side {
        // The reserved ID sorted before native IDs when sizes tied.
        let position = unique.partition_point(|&(other_size, _, _)| other_size > size);
        unique.insert(position, (size, None, vec![crate::blending::CONTROL_ID]));
    }
    let mut side = 128_usize;
    let placements = loop {
        let (mut x, mut y, mut row) = (0, 0, 0);
        let mut placements = vec![];
        let mut fits = true;
        for &(size, _, _) in &unique {
            if x + size > side {
                x = 0;
                y += row;
                row = 0;
            }
            if y + size > side || size > side {
                fits = false;
                break;
            }
            placements.push([x, y, size, size]);
            x += size;
            row = row.max(size);
        }
        if fits {
            break placements;
        }
        side *= 2;
        ensure!(
            side <= 8192,
            "Native textures exceed one 8192px atlas; no resize is permitted"
        );
    };
    // Crop only unused atlas space. Keep every packed image (including its
    // transparent pixels), rectangle origin and native dimensions unchanged.
    let width = placements
        .iter()
        .map(|r| r[0] + r[2])
        .max()
        .unwrap_or(2)
        .max(2)
        .next_power_of_two();
    let height = placements
        .iter()
        .map(|r| r[1] + r[3])
        .max()
        .unwrap_or(2)
        .max(2)
        .next_power_of_two();
    let mut pixels = vec![0_u8; width * height * 4];
    let mut images = vec![];
    let mut ids = BTreeMap::new();
    for ((_, source_image, source_ids), rect) in unique.into_iter().zip(placements) {
        for id in &source_ids {
            let _previous = ids.insert(*id, rect);
        }
        let Some(image) = source_image else { continue };
        for y in 0..image.size {
            for x in 0..image.size {
                let dst = ((rect[1] + y) * width + rect[0] + x) * 4;
                pixels[dst..dst + 4].copy_from_slice(&image.pixels[y * image.size + x]);
            }
        }
        let bytes = image.pixels.iter().flatten().copied().collect::<Vec<_>>();
        images.push(AtlasImage {
            source_ids,
            rect,
            crc: crc32fast::hash(&bytes),
        });
    }
    Ok((pixels, [width, height], images, ids))
}
/// Convert one region to native triangles and a shared texture atlas.
#[allow(
    clippy::cognitive_complexity,
    reason = "mirror native shape and opcode dispatch without changing its evaluation"
)]
#[allow(
    clippy::too_many_lines,
    reason = "keep native opcode dispatch and ordered conversion stages together"
)]
#[allow(
    clippy::indexing_slicing,
    reason = "indices follow validated native tables and allocated image dimensions"
)]
#[allow(
    clippy::float_cmp,
    reason = "exact native vertex and material identity; approximate equality would merge distinct data"
)]
#[allow(
    clippy::print_stderr,
    reason = "the CLI reports errors and decoding progress to stderr"
)]
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
#[allow(
    clippy::shadow_unrelated,
    reason = "each loop or conversion stage scopes its own intermediate values"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::float_arithmetic,
    clippy::suboptimal_flops,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
pub(crate) fn build(cache: &mut Cache, world: &World, scale: f32) -> Result<RegionMesh> {
    let under = cache
        .group(2, 1)?
        .into_iter()
        .map(|(id, b)| Ok(((id + 1) as u8, Floor::decode(&b, false)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let over = cache
        .group(2, 4)?
        .into_iter()
        .map(|(id, b)| Ok(((id + 1) as u8, Floor::decode(&b, true)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut textures = Textures::new(cache)?;
    let mut needed = std::collections::BTreeSet::new();
    for layer in 0..5 {
        let uw = layer == 4;
        let p = if uw { 0 } else { layer };
        for x in 0_i32..64_i32 {
            for y in 0_i32..64_i32 {
                let tile = if uw {
                    let Some(t) = world.underwater_tile(x, y) else {
                        continue;
                    };
                    t
                } else {
                    world.tile(p, x, y)
                };
                for (overlay, id) in [(false, tile.under), (true, tile.over)] {
                    if id != 0 {
                        let f = (if overlay { &over } else { &under })
                            .get(&id)
                            .context("Missing floor definition")?;
                        if f.texture >= 0_i32 {
                            let _consumed = needed.insert(f.texture);
                        }
                    }
                }
            }
        }
    }
    let mut blending = crate::blending::Blending::build(world, &under, &over);
    for floor in blending.floors.values() {
        if floor.texture >= 0_i32 {
            let _consumed = needed.insert(floor.texture);
        }
    }
    for &id in &needed {
        textures.load(cache, id)?;
    }
    textures.images.retain(|id, _| needed.contains(id));
    blending.tints = blending
        .floors
        .iter()
        .map(|(&id, f)| (id, f.tint(textures.images.get(&f.texture))))
        .collect();
    let control_side = blending.control_side()?;
    let (mut pixels, [width, height], atlas_images, rects) = pack(&textures, Some(control_side))?;
    blending.rect = rects[&crate::blending::CONTROL_ID];
    let controls = blending.encode(&rects)?;
    ensure!(
        controls.len() <= control_side * control_side,
        "Blend controls exceed their reserved rectangle"
    );
    for (y, row) in controls.chunks(control_side).enumerate() {
        let dst = ((blending.rect[1] + y) * width + blending.rect[0]) * 4;
        let bytes = row.as_flattened();
        pixels[dst..dst + bytes.len()].copy_from_slice(bytes);
    }
    let mut materials: Vec<Surface> = vec![];
    let mut triangles = vec![];
    let mut tiles = 0;
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    let mut plane_triangles = [0; 4];
    let mut underwater_triangles = 0;
    for layer in 0..5 {
        let uw = layer == 4;
        let p = if uw { 0 } else { layer };
        for x in 0_i32..64_i32 {
            for y in 0_i32..64_i32 {
                let tile = if uw {
                    let Some(t) = world.underwater_tile(x, y) else {
                        continue;
                    };
                    t
                } else {
                    world.tile(p, x, y)
                };
                if tile.under == 0 && tile.over == 0 {
                    continue;
                }
                tiles += 1;
                let shape = if tile.over == 0 {
                    0
                } else {
                    tile.shape as usize + 1
                };
                ensure!(shape < 13, "Invalid tile shape");
                let points = terrain::VERTICES[shape]
                    .iter()
                    .map(|&c| terrain::vertex(c, tile.rotation))
                    .collect::<Vec<_>>();
                let corners = [
                    (x, y),
                    (x + 1_i32, y),
                    (x + 1_i32, y + 1_i32),
                    (x, y + 1_i32),
                ];
                let heights = corners.map(|(x, y)| world.layer_tile(uw, p, x, y).height);
                let surface_heights = corners.map(|(x, y)| world.tile(0, x, y).height);
                let normals = corners.map(|(x, y)| world.layer_normal(uw, p, x, y));
                for f in terrain::FACES[shape].as_chunks::<4>().0 {
                    let overlay = f[0] == 1;
                    let id = if overlay { tile.over } else { tile.under };
                    if id == 0 {
                        continue;
                    }
                    let floor = (if overlay { &over } else { &under })
                        .get(&id)
                        .context("Missing face floor")?;
                    if floor.rgb == 0x00ff_00ff && floor.texture < 0_i32 {
                        continue;
                    }
                    let texture = textures.images.get(&floor.texture);
                    let water = texture.is_some_and(|t| t.effect == 4) && !uw;
                    let tint = floor.tint(texture);
                    let surface = Surface {
                        rect: rects.get(&floor.texture).copied().unwrap_or([0; 4]),
                        tint,
                        texture: floor.texture,
                        water,
                        scale: floor.scale,
                    };
                    let material = materials
                        .iter()
                        .position(|m| {
                            m.rect == surface.rect
                                && m.tint == surface.tint
                                && m.water == water
                                && m.scale == surface.scale
                        })
                        .unwrap_or_else(|| {
                            let i = materials.len();
                            materials.push(surface);
                            i
                        });
                    ensure!(
                        material < 64,
                        "Atlas shader supports at most 64 native material bindings"
                    );
                    let vertices = core::array::from_fn(|i| {
                        let index = f[i + 1];
                        let (xy, w) = points[if index < 4 {
                            (index + 4 - tile.rotation as usize) & 3
                        } else {
                            index
                        }];
                        let vertex_heights = if uw
                            && !overlay
                            && tile.over != 0
                            && terrain::shoreline_vertex(tile.shape, tile.rotation, xy)
                        {
                            surface_heights
                        } else {
                            heights
                        };
                        let z = terrain::hd_height(vertex_heights, xy);
                        let unit = 0.5 * scale;
                        let pos = [
                            (x as f32 + xy[0] - 32.) * 128. * unit,
                            (y as f32 + xy[1] - 32.) * 128. * unit,
                            z * unit,
                        ];
                        let normal = terrain::normalize(core::array::from_fn(|c| {
                            (0..4).map(|j| normals[j][c] * w[j]).sum()
                        }));
                        // Preserve the client's world-space UV frequency AND phase. Only
                        // subtract whole repeats to keep the Source float UV range small.
                        let phase = [
                            (world.x as f32 * 8192. / floor.scale).fract(),
                            (world.y as f32 * 8192. / floor.scale).fract(),
                        ];
                        let uv = [
                            (x as f32 + xy[0]) * 128. / floor.scale + phase[0],
                            (y as f32 + xy[1]) * 128. / floor.scale + phase[1],
                        ];
                        for c in 0..3 {
                            bounds[0][c] = bounds[0][c].min(pos[c]);
                            bounds[1][c] = bounds[1][c].max(pos[c]);
                        }
                        Vertex { pos, normal, uv }
                    });
                    let [a, b, c] = vertices;
                    let area = (b.pos[0] - a.pos[0]) * (c.pos[1] - a.pos[1])
                        - (b.pos[1] - a.pos[1]) * (c.pos[0] - a.pos[0]);
                    let vertices = if area < 0. { [a, c, b] } else { [a, b, c] };
                    triangles.push(Triangle {
                        vertices,
                        water,
                        material,
                        underwater: uw,
                        layer: layer as u8,
                        blend_address: {
                            let a = controls[(layer * 64 + x as usize) * 64 + y as usize];
                            u32::from(a[0]) + u32::from(a[1]) * 256 + u32::from(a[2]) * 65536
                        },
                    });
                    if uw {
                        underwater_triangles += 1;
                    } else {
                        plane_triangles[p] += 1;
                    }
                }
            }
        }
    }
    ensure!(!triangles.is_empty(), "No native terrain geometry");
    eprintln!("Native geometry: {} surface + {} underwater triangles; {} exact unique images, {}x{}px atlas, no padding",plane_triangles.iter().sum::<usize>(),underwater_triangles,atlas_images.len(),width,height);
    Ok(RegionMesh {
        triangles,
        pixels,
        width,
        height,
        tiles,
        unique_tiles: atlas_images.len(),
        bounds,
        plane_triangles,
        underwater_triangles,
        materials,
        atlas_images,
        blending,
        mapping: [
            2. / scale,
            4096.,
            world.x as f32 * 8192.,
            world.y as f32 * 8192.,
        ],
    })
}
#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::modulo_arithmetic,
    clippy::unwrap_used,
    reason = "test fixtures use exact values, fixed indexes and panic on unexpected failures"
)]
mod tests {
    use super::*;
    #[test]
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::integer_division,
        clippy::integer_division_remainder_used,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    fn atlas_deduplicates_exact_images_without_padding_or_resizing() {
        let original = (0_i32..128_i32 * 128_i32)
            .map(|i| [(i % 128_i32) as u8, (i / 128_i32) as u8, 42, 255])
            .collect::<Vec<_>>();
        let small = vec![[11, 22, 33, 44]; 64 * 64];
        let textures = Textures::fixture(BTreeMap::from([
            (1_i32, (128, original.clone())),
            (2_i32, (128, original.clone())),
            (3_i32, (64, small.clone())),
        ]));
        let (pixels, [width, height], images, ids) = pack(&textures, None).unwrap();
        assert_eq!((width, height), (256, 128));
        assert_eq!(pixels.len(), width * height * 4);
        assert_eq!(images.len(), 2);
        assert_eq!(ids[&1_i32], ids[&2_i32]);
        assert_eq!(ids[&1_i32][2..], [128, 128]);
        assert_eq!(ids[&3_i32][2..], [64, 64]);
        for (id, size, expected) in [(1_i32, 128, &original), (3_i32, 64, &small)] {
            let rect = ids[&id];
            for y in 0..size {
                for x in 0..size {
                    let at = ((rect[1] + y) * width + rect[0] + x) * 4;
                    assert_eq!(&pixels[at..at + 4], &expected[y * size + x]);
                }
            }
        }
        assert_eq!(ids[&3_i32][0], ids[&1_i32][0] + 128); // directly adjacent, no gutter
    }
    #[test]
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    fn crop_dimensions_follow_occupied_rectangles_for_small_and_large_atlases() {
        for (count, size, expected) in [
            (1_i32, 2, [2, 2]),
            (2_i32, 2, [4, 2]),
            (19_i32, 128, [1024, 512]),
            (65_i32, 128, [2048, 1024]),
        ] {
            let textures = Textures::fixture(
                (0_i32..count)
                    .map(|id| {
                        // Transparent images are still retained: crop only free space,
                        // not pixels inside a source image's repeating UV domain.
                        (id, (size, vec![[id as u8, 13, 17, 0]; size * size]))
                    })
                    .collect(),
            );
            let (pixels, dimensions, images, _) = pack(&textures, None).unwrap();
            assert_eq!(dimensions, expected);
            assert_eq!(pixels.len(), dimensions[0] * dimensions[1] * 4);
            for image in images {
                assert!(image.rect[0] + image.rect[2] <= dimensions[0]);
                assert!(image.rect[1] + image.rect[3] <= dimensions[1]);
                assert_eq!(&image.rect[2..], &[size, size]);
            }
        }
    }
    #[test]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    fn unsigned_overlay_id() {
        let f = Floor::decode(&[1, 0x60, 0x60, 0x58, 3, 1, 45, 0], true).unwrap();
        assert_eq!(f.texture, 301_i32);
    }
}
