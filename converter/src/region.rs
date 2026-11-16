// SPDX-License-Identifier: AGPL-3.0-only
//! Native terrain geometry and an exact-copy texture atlas. No rasterized tiles.
use crate::{
    binary::Reader,
    cache::Cache,
    terrain::{self, World},
    textures::Textures,
};
use anyhow::{bail, ensure, Context, Result};
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub struct Floor {
    pub rgb: u32,
    pub texture: i32,
    pub scale: f32,
    pub blend: bool,
}
impl Floor {
    fn decode(data: &[u8], overlay: bool) -> Result<Self> {
        let mut r = Reader::new(data);
        let mut f = Self {
            rgb: 0,
            texture: -1,
            scale: 128.,
            blend: false,
        };
        loop {
            let op = r.u8()?;
            match (overlay, op) {
                (_, 0) => break,
                (_, 1) => f.rgb = r.u24()?,
                (false, 2) | (true, 3) => {
                    let id = r.u16()?;
                    f.texture = if id == 65535 { -1 } else { id as i32 };
                }
                (false, 3) | (true, 9) => f.scale = r.u16()? as f32,
                (true, 2) => f.texture = r.u8()? as i32,
                (false, 4) | (true, 5) | (true, 8) | (true, 10) => {}
                (true, 7) | (true, 13) => {
                    r.u24()?;
                }
                (true, 11) | (true, 14) => {
                    r.u8()?;
                }
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
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}
#[derive(Clone, Debug)]
pub struct Triangle {
    pub vertices: [Vertex; 3],
    pub water: bool,
    pub material: usize,
    pub underwater: bool,
}
#[derive(Clone, serde::Serialize)]
pub struct Surface {
    pub rect: [usize; 4],
    pub tint: [f32; 4],
    pub texture: i32,
    pub water: bool,
}
#[derive(serde::Serialize)]
pub struct AtlasImage {
    pub source_ids: Vec<i32>,
    pub rect: [usize; 4],
    pub crc: u32,
}
pub struct RegionMesh {
    pub triangles: Vec<Triangle>,
    pub pixels: Vec<u8>,
    pub size: usize,
    pub tiles: usize,
    pub unique_tiles: usize,
    pub bounds: [[f32; 3]; 2],
    pub plane_triangles: [usize; 4],
    pub underwater_triangles: usize,
    pub materials: Vec<Surface>,
    pub atlas_images: Vec<AtlasImage>,
}
/// Deduplicate ONLY exact native images, retaining dimensions and every RGBA byte.
type PackedAtlas = (Vec<u8>, usize, Vec<AtlasImage>, BTreeMap<i32, [usize; 4]>);
fn pack(textures: &Textures) -> Result<PackedAtlas> {
    let mut unique: Vec<(&crate::textures::Texture, Vec<i32>)> = vec![];
    for (&id, image) in &textures.images {
        ensure!(
            image.pixels.len() == image.size * image.size,
            "Invalid native texture dimensions"
        );
        if let Some((_, ids)) = unique
            .iter_mut()
            .find(|(other, _)| other.size == image.size && other.pixels == image.pixels)
        {
            ids.push(id);
        } else {
            unique.push((image, vec![id]));
        }
    }
    // Power-of-two native images pack on shelves, without gutters or extrusion.
    unique.sort_by_key(|(t, _)| std::cmp::Reverse(t.size));
    let mut side = 128usize;
    let placements = loop {
        let (mut x, mut y, mut row) = (0, 0, 0);
        let mut placements = vec![];
        let mut fits = true;
        for (image, _) in &unique {
            if x + image.size > side {
                x = 0;
                y += row;
                row = 0;
            }
            if y + image.size > side || image.size > side {
                fits = false;
                break;
            }
            placements.push([x, y, image.size, image.size]);
            x += image.size;
            row = row.max(image.size);
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
    let mut pixels = vec![0u8; side * side * 4];
    let mut images = vec![];
    let mut ids = BTreeMap::new();
    for ((image, source_ids), rect) in unique.into_iter().zip(placements) {
        for y in 0..image.size {
            for x in 0..image.size {
                let dst = ((rect[1] + y) * side + rect[0] + x) * 4;
                pixels[dst..dst + 4].copy_from_slice(&image.pixels[y * image.size + x]);
            }
        }
        for id in &source_ids {
            ids.insert(*id, rect);
        }
        let bytes = image.pixels.iter().flatten().copied().collect::<Vec<_>>();
        images.push(AtlasImage {
            source_ids,
            rect,
            crc: crc32fast::hash(&bytes),
        });
    }
    Ok((pixels, side, images, ids))
}
pub fn build(cache: &mut Cache, world: &World, scale: f32) -> Result<RegionMesh> {
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
        for x in 0..64 {
            for y in 0..64 {
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
                        if f.texture >= 0 {
                            needed.insert(f.texture);
                        }
                    }
                }
            }
        }
    }
    for &id in &needed {
        textures.load(cache, id)?;
    }
    textures.images.retain(|id, _| needed.contains(id));
    let (pixels, size, atlas_images, rects) = pack(&textures)?;
    let mut materials: Vec<Surface> = vec![];
    let mut triangles = vec![];
    let mut tiles = 0;
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    let mut plane_triangles = [0; 4];
    let mut underwater_triangles = 0;
    for layer in 0..5 {
        let uw = layer == 4;
        let p = if uw { 0 } else { layer };
        for x in 0..64 {
            for y in 0..64 {
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
                let corners = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
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
                    if floor.rgb == 0xff00ff && floor.texture < 0 {
                        continue;
                    }
                    let texture = textures.images.get(&floor.texture);
                    let water = texture.is_some_and(|t| t.effect == 4) && !uw;
                    let mut tint = [1.; 4];
                    for (c, shift) in [(0, 16), (1, 8), (2, 0)] {
                        let base = (floor.rgb >> shift & 255) as f32;
                        let brightness = texture.map_or(0., |t| t.brightness as f32);
                        let boost = texture.map_or(0., |t| t.boost as f32);
                        tint[c] = (base * (256. - brightness) + 255. * brightness) / 256. / 255.
                            * (256. + boost)
                            / 256.;
                    }
                    let surface = Surface {
                        rect: rects.get(&floor.texture).copied().unwrap_or([0; 4]),
                        tint,
                        texture: floor.texture,
                        water,
                    };
                    let material = if let Some(i) = materials.iter().position(|m| {
                        m.rect == surface.rect && m.tint == surface.tint && m.water == water
                    }) {
                        i
                    } else {
                        let i = materials.len();
                        materials.push(surface);
                        i
                    };
                    ensure!(
                        material < 64,
                        "Atlas shader supports at most 64 native material bindings"
                    );
                    let vertices = std::array::from_fn(|i| {
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
                        let normal = terrain::normalize(std::array::from_fn(|c| {
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
    eprintln!("Native geometry: {} surface + {} underwater triangles; {} exact unique images, {}px atlas, no padding",plane_triangles.iter().sum::<usize>(),underwater_triangles,atlas_images.len(),size);
    Ok(RegionMesh {
        triangles,
        pixels,
        size,
        tiles,
        unique_tiles: atlas_images.len(),
        bounds,
        plane_triangles,
        underwater_triangles,
        materials,
        atlas_images,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atlas_deduplicates_exact_images_without_padding_or_resizing() {
        let original = (0..128 * 128)
            .map(|i| [(i % 128) as u8, (i / 128) as u8, 42, 255])
            .collect::<Vec<_>>();
        let small = vec![[11, 22, 33, 44]; 64 * 64];
        let textures = Textures::fixture(BTreeMap::from([
            (1, (128, original.clone())),
            (2, (128, original.clone())),
            (3, (64, small.clone())),
        ]));
        let (pixels, side, images, ids) = pack(&textures).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(ids[&1], ids[&2]);
        assert_eq!(ids[&1][2..], [128, 128]);
        assert_eq!(ids[&3][2..], [64, 64]);
        for (id, size, expected) in [(1, 128, &original), (3, 64, &small)] {
            let rect = ids[&id];
            for y in 0..size {
                for x in 0..size {
                    let at = ((rect[1] + y) * side + rect[0] + x) * 4;
                    assert_eq!(&pixels[at..at + 4], &expected[y * size + x]);
                }
            }
        }
        assert_eq!(ids[&3][0], ids[&1][0] + 128); // directly adjacent, no gutter
    }
    #[test]
    fn unsigned_overlay_id() {
        let f = Floor::decode(&[1, 0x60, 0x60, 0x58, 3, 1, 45, 0], true).unwrap();
        assert_eq!(f.texture, 301);
    }
}
