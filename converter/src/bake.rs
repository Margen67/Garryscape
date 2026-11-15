// SPDX-License-Identifier: AGPL-3.0-only
//! Bake floor blending into one atlas. Nothing in this module runs in GMod.
use crate::{
    binary::Reader,
    cache::Cache,
    terrain::{self, World},
    textures::Textures,
};
use anyhow::{bail, ensure, Context, Result};
use std::collections::{BTreeMap, HashMap};
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
}
pub struct Baked {
    pub triangles: Vec<Triangle>,
    pub pixels: Vec<u8>,
    pub size: usize,
    pub tiles: usize,
    pub unique_tiles: usize,
    pub bounds: [[f32; 3]; 2],
    pub plane_triangles: [usize; 4],
}
type ShapedVertex = ([f32; 2], [f32; 4]);
type ShapedFace = ([ShapedVertex; 3], bool);

#[derive(Clone)]
struct Patch {
    p: usize,
    x: i32,
    y: i32,
    faces: Vec<ShapedFace>,
    image: usize,
}
struct Baker<'a> {
    world: &'a World,
    under: BTreeMap<u8, Floor>,
    over: BTreeMap<u8, Floor>,
    textures: Textures,
}
impl<'a> Baker<'a> {
    fn floor(&self, overlay: bool, id: u8) -> Result<&Floor> {
        (if overlay { &self.over } else { &self.under })
            .get(&id)
            .with_context(|| format!("Missing floor {overlay}/{id}"))
    }
    fn water(&self, f: &Floor) -> bool {
        self.textures
            .images
            .get(&f.texture)
            .is_some_and(|t| t.effect == 4)
    }
    fn sample(&self, f: &Floor, x: f32, y: f32) -> [f32; 4] {
        if let Some(tex) = self.textures.images.get(&f.texture) {
            let u = (x * 128. / f.scale).rem_euclid(1.);
            let v = (y * 128. / f.scale).rem_euclid(1.);
            let px = tex.pixels[(v * 128.) as usize * 128 + (u * 128.) as usize];
            let mut c = px.map(|v| v as f32);
            // RT4 getTexturedColor combines the floor colour with grayscale
            // using the material brightness, then multiplies the texture.
            for (channel, shift) in [(0, 16), (1, 8), (2, 0)] {
                let base = ((f.rgb >> shift) & 255) as f32;
                let tint =
                    (base * (256. - tex.brightness as f32) + 255. * tex.brightness as f32) / 256.;
                c[channel] *= tint / 255. * (256. + tex.boost as f32) / 256.;
            }
            if self.water(f) {
                c[3] = c[3].clamp(100., 210.);
            } else {
                c[3] = 255.;
            }
            c
        } else {
            [
                (f.rgb >> 16 & 255) as f32,
                (f.rgb >> 8 & 255) as f32,
                (f.rgb & 255) as f32,
                255.,
            ]
        }
    }
    fn corner(&self, p: usize, x: i32, y: i32) -> Result<BTreeMap<(bool, u8), f32>> {
        let mut values = BTreeMap::new();
        for dx in -1..=0 {
            for dy in -1..=0 {
                let t = self.world.tile(p, x + dx, y + dy);
                let mut key = None;
                if t.over != 0 {
                    let f = self.floor(true, t.over)?;
                    if f.blend && !self.water(f) {
                        let shape = t.shape as usize + 1;
                        let corner = match (-dx, -dy) {
                            (0, 0) => 0,
                            (1, 0) => 1,
                            (1, 1) => 2,
                            _ => 3,
                        };
                        let corner = (corner + t.rotation as usize) & 3;
                        let covered = terrain::FACES[shape]
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|face| face[0] == 1 && face[1..].contains(&corner));
                        if covered {
                            key = Some((true, t.over));
                        }
                    }
                }
                if key.is_none() && t.under != 0 {
                    key = Some((false, t.under));
                }
                if let Some(k) = key {
                    *values.entry(k).or_default() += 1.;
                }
            }
        }
        let total = values.values().sum::<f32>();
        if total > 0. {
            for v in values.values_mut() {
                *v /= total;
            }
        }
        Ok(values)
    }
    fn blend_sample(
        &self,
        corners: &[BTreeMap<(bool, u8), f32>; 4],
        x: f32,
        y: f32,
        wx: f32,
        wy: f32,
    ) -> Result<[f32; 4]> {
        let weights = [(1. - x) * (1. - y), x * (1. - y), x * y, (1. - x) * y];
        let mut out = [0.; 4];
        let mut total = 0.;
        for (i, corner) in corners.iter().enumerate() {
            for (&(over, id), &amount) in corner {
                let w = weights[i] * amount;
                let c = self.sample(self.floor(over, id)?, wx, wy);
                for k in 0..4 {
                    out[k] += c[k] * w;
                }
                total += w;
            }
        }
        if total > 0. {
            for v in &mut out {
                *v /= total;
            }
        }
        Ok(out)
    }
}
fn barycentric(v: [[f32; 2]; 3], p: [f32; 2]) -> Option<[f32; 3]> {
    let d = (v[1][1] - v[2][1]) * (v[0][0] - v[2][0]) + (v[2][0] - v[1][0]) * (v[0][1] - v[2][1]);
    if d.abs() < 1e-7 {
        return None;
    }
    let a = ((v[1][1] - v[2][1]) * (p[0] - v[2][0]) + (v[2][0] - v[1][0]) * (p[1] - v[2][1])) / d;
    let b = ((v[2][1] - v[0][1]) * (p[0] - v[2][0]) + (v[0][0] - v[2][0]) * (p[1] - v[2][1])) / d;
    let c = 1. - a - b;
    if a >= -0.0001 && b >= -0.0001 && c >= -0.0001 {
        Some([a, b, c])
    } else {
        None
    }
}
pub fn build(cache: &mut Cache, world: &World, scale: f32) -> Result<Baked> {
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
    let mut baker = Baker {
        world,
        under,
        over,
        textures: Textures::new(cache)?,
    };
    let mut required = std::collections::BTreeSet::new();
    for p in 0..4 {
        for x in -1..=64 {
            for y in -1..=64 {
                let t = world.tile(p, x, y);
                for (overlay, id) in [(false, t.under), (true, t.over)] {
                    if id != 0 {
                        let f = baker.floor(overlay, id)?;
                        if f.texture >= 0 {
                            required.insert(f.texture);
                        }
                    }
                }
            }
        }
    }
    for id in required {
        baker.textures.load(cache, id)?;
    }
    let mut patches = vec![];
    let mut images: Vec<Vec<u8>> = vec![];
    let mut hashes: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut tiles = 0;
    for p in 0..4 {
        for x in 0..64 {
            for y in 0..64 {
                let tile = world.tile(p, x, y);
                if tile.under == 0 && tile.over == 0 {
                    continue;
                }
                let shape = if tile.over == 0 {
                    0
                } else {
                    tile.shape as usize + 1
                };
                ensure!(shape < 13, "Invalid tile shape");
                let points = terrain::VERTICES[shape]
                    .iter()
                    .map(|&code| terrain::vertex(code, tile.rotation))
                    .collect::<Vec<_>>();
                let mut faces = vec![];
                for f in terrain::FACES[shape].as_chunks::<4>().0 {
                    let overlay = f[0] == 1;
                    let id = if overlay { tile.over } else { tile.under };
                    if id == 0 {
                        continue;
                    }
                    let floor = baker.floor(overlay, id)?;
                    if floor.rgb == 0xff00ff && floor.texture < 0 {
                        continue;
                    }
                    let v = std::array::from_fn(|i| {
                        let idx = f[i + 1];
                        points[if idx < 4 {
                            (idx + 4 - tile.rotation as usize) & 3
                        } else {
                            idx
                        }]
                    });
                    faces.push((v, overlay));
                }
                if faces.is_empty() {
                    continue;
                }
                let corners = [
                    baker.corner(p, x, y)?,
                    baker.corner(p, x + 1, y)?,
                    baker.corner(p, x + 1, y + 1)?,
                    baker.corner(p, x, y + 1)?,
                ];
                let mut image = vec![0u8; 128 * 128 * 4];
                for py in 0..128 {
                    for px in 0..128 {
                        let (u, v) = ((px as f32 + 0.5) / 128., (py as f32 + 0.5) / 128.);
                        let overlay = faces
                            .iter()
                            .find(|(verts, _)| barycentric(verts.map(|v| v.0), [u, v]).is_some())
                            .map(|(_, over)| *over);
                        let fallback = tile.over != 0 && tile.under == 0;
                        let is_over = overlay.unwrap_or(fallback);
                        let id = if is_over { tile.over } else { tile.under };
                        let c = if id == 0 {
                            [0.; 4]
                        } else {
                            let floor = baker.floor(is_over, id)?;
                            if (!is_over || (floor.blend && !baker.water(floor)))
                                && !corners.iter().all(|c| c.is_empty())
                            {
                                baker.blend_sample(
                                    &corners,
                                    u,
                                    v,
                                    world.x as f32 * 64. + x as f32 + u,
                                    world.y as f32 * 64. + y as f32 + v,
                                )?
                            } else {
                                baker.sample(
                                    floor,
                                    world.x as f32 * 64. + x as f32 + u,
                                    world.y as f32 * 64. + y as f32 + v,
                                )
                            }
                        };
                        let offset = (py * 128 + px) * 4;
                        for k in 0..4 {
                            image[offset + k] = c[k].round().clamp(0., 255.) as u8;
                        }
                    }
                }
                let hash = crc32fast::hash(&image);
                let found = hashes
                    .get(&hash)
                    .and_then(|ids| ids.iter().find(|&&i| images[i] == image))
                    .copied();
                let idx = if let Some(id) = found {
                    id
                } else {
                    let i = images.len();
                    images.push(image);
                    hashes.entry(hash).or_default().push(i);
                    i
                };
                patches.push(Patch {
                    p,
                    x,
                    y,
                    faces,
                    image: idx,
                });
                tiles += 1;
            }
        }
        eprintln!(
            "Baked floor {p}: {tiles} tiles total; {} unique texture patches",
            images.len()
        );
    }
    // Keep the original 128x128 samples. Sixteen-pixel gutters prevent
    // neighboring atlas islands bleeding together through common mip levels.
    let gutter = 16;
    let pitch = 128 + 2 * gutter;
    let mut size = 256usize;
    while (size / pitch) * (size / pitch) < images.len() {
        size *= 2;
    }
    ensure!(size<=8192,"Single atlas needs {size}px ({} distinct tiles); exceeds the 8192px Source target. No texture resolution was silently reduced.",images.len());
    let cols = size / pitch;
    let mut pixels = vec![0u8; size * size * 4];
    for (i, image) in images.iter().enumerate() {
        let ox = i % cols * pitch;
        let oy = i / cols * pitch;
        for y in 0..pitch {
            for x in 0..pitch {
                let src = ((y as i32 - gutter as i32).clamp(0, 127) as usize * 128
                    + (x as i32 - gutter as i32).clamp(0, 127) as usize)
                    * 4;
                let dst = ((oy + y) * size + ox + x) * 4;
                pixels[dst..dst + 4].copy_from_slice(&image[src..src + 4]);
            }
        }
    }
    let mut triangles = vec![];
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    let mut plane_triangles = [0; 4];
    let unit = 0.5 * scale;
    for patch in patches {
        let (p, x, y) = (patch.p, patch.x, patch.y);
        let corners = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
        let heights = corners.map(|(x, y)| world.height(p, x, y));
        let normals = corners.map(|(x, y)| world.normal(p, x, y));
        let tile = world.tile(p, x, y);
        for (face, over) in patch.faces {
            let floor = baker.floor(over, if over { tile.over } else { tile.under })?;
            let vertices = face.map(|(xy, w)| {
                let z = (0..4).map(|i| w[i] * heights[i]).sum::<f32>();
                let normal = terrain::normalize(std::array::from_fn(|c| {
                    (0..4).map(|i| w[i] * normals[i][c]).sum()
                }));
                let pos = [
                    (x as f32 + xy[0] - 32.) * 128. * unit,
                    (y as f32 + xy[1] - 32.) * 128. * unit,
                    z * unit,
                ];
                let uv = [
                    (patch.image % cols * pitch + gutter) as f32 / size as f32
                        + xy[0] * 128. / size as f32,
                    (patch.image / cols * pitch + gutter) as f32 / size as f32
                        + xy[1] * 128. / size as f32,
                ];
                for c in 0..3 {
                    bounds[0][c] = bounds[0][c].min(pos[c]);
                    bounds[1][c] = bounds[1][c].max(pos[c]);
                }
                Vertex { pos, normal, uv }
            });
            // Shaped floor tables are clockwise when viewed from above in Source XY.
            let [a, b, c] = vertices;
            let area = (b.pos[0] - a.pos[0]) * (c.pos[1] - a.pos[1])
                - (b.pos[1] - a.pos[1]) * (c.pos[0] - a.pos[0]);
            let vertices = if area < 0. { [a, c, b] } else { [a, b, c] };
            triangles.push(Triangle {
                vertices,
                water: baker.water(floor),
            });
            plane_triangles[p] += 1;
        }
    }
    ensure!(!triangles.is_empty(), "No visible terrain");
    Ok(Baked {
        triangles,
        pixels,
        size,
        tiles,
        unique_tiles: images.len(),
        bounds,
        plane_triangles,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn triangle_coverage() {
        assert!(barycentric([[0., 0.], [1., 0.], [0., 1.]], [0.2, 0.2]).is_some());
        assert!(barycentric([[0., 0.], [1., 0.], [0., 1.]], [0.8, 0.8]).is_none());
    }
    #[test]
    fn unsigned_overlay_id() {
        let f = Floor::decode(&[1, 0x60, 0x60, 0x58, 3, 1, 45, 0], true).unwrap();
        assert_eq!(f.texture, 301);
    }
}
