// SPDX-License-Identifier: AGPL-3.0-only
//! Revision 530 tile decoding and shaped floor geometry. Shape tables adapted from
//! dennisdev/rs-map-viewer (BSD-2-Clause; see third_party/rs-map-viewer-LICENSE).
use crate::{binary::Reader, cache::Cache};
use anyhow::{bail, ensure, Context, Result};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Default, Debug)]
pub struct Tile {
    pub height: i32,
    pub under: u8,
    pub over: u8,
    pub shape: u8,
    pub rotation: u8,
    pub settings: u8,
}
pub struct Region {
    pub tiles: Vec<Tile>,
    pub underwater: Option<Vec<Tile>>,
}
impl Region {
    pub fn tile(&self, p: usize, x: usize, y: usize) -> Tile {
        self.tiles[p * 4096 + x * 64 + y]
    }
    pub fn decode(bytes: &[u8], rx: i32, ry: i32) -> Result<Self> {
        Self::decode_layer(bytes, rx, ry, None)
    }
    fn decode_layer(bytes: &[u8], rx: i32, ry: i32, surface: Option<&[Tile]>) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let planes = if surface.is_some() { 1 } else { 4 };
        let mut tiles = vec![Tile::default(); planes * 4096];
        for p in 0..planes {
            for x in 0..64 {
                for y in 0..64 {
                    let i = p * 4096 + x * 64 + y;
                    let previous = if let Some(base) = surface {
                        base[i].height
                    } else if p == 0 {
                        0
                    } else {
                        tiles[i - 4096].height
                    };
                    let mut ended = false;
                    for _ in 0..64 {
                        let op = r.u8()?;
                        match op {
                            0 => {
                                tiles[i].height = if surface.is_some() {
                                    previous
                                } else if p == 0 {
                                    ground_height(rx * 64 + x as i32, ry * 64 + y as i32)
                                } else {
                                    previous - 240
                                };
                                ended = true;
                                break;
                            }
                            1 => {
                                let h = r.u8()?;
                                tiles[i].height = if surface.is_some() {
                                    previous + h as i32 * 8
                                } else {
                                    previous - if h == 1 { 0 } else { h as i32 * 8 }
                                };
                                ended = true;
                                break;
                            }
                            2..=49 => {
                                tiles[i].over = r.u8()?;
                                tiles[i].shape = (op - 2) / 4;
                                tiles[i].rotation = (op - 2) & 3
                            }
                            50..=81 => tiles[i].settings = op - 49,
                            _ => tiles[i].under = op - 81,
                        }
                    }
                    ensure!(ended, "Tile attribute limit at {p}/{x}/{y}");
                }
            }
        }
        while r.position < bytes.len() {
            match r.u8()? {
                0 => {
                    let flags = r.u8()?;
                    ensure!(flags & 128 == 0, "Unknown environment flags");
                    for (flag, len) in [(1, 4), (2, 2), (4, 2), (8, 2), (16, 6), (32, 4), (64, 2)] {
                        if flags & flag != 0 {
                            r.take(len)?;
                        }
                    }
                }
                1 => {
                    let count = r.u8()?;
                    for _ in 0..count {
                        r.u8()?;
                        r.take(6)?;
                        let radius = r.u8()? as usize;
                        ensure!(radius <= 127, "Light radius too large");
                        r.take((2 * radius + 1) * 2)?;
                        r.u16()?;
                        let func = r.u8()?;
                        if func & 31 == 31 {
                            r.u16()?;
                        }
                    }
                }
                129 => {
                    for _ in 0..4 {
                        match r.u8()? {
                            0 | 2 => {}
                            1 => {
                                r.take(256)?;
                            }
                            n => bail!("Unknown shadow map mode {n}"),
                        }
                    }
                }
                n => bail!("Unknown map extension {n} at {}", r.position - 1),
            }
        }
        Ok(Self {
            tiles,
            underwater: None,
        })
    }
}
fn named(cache: &mut Cache, name: &str) -> Result<Option<Vec<u8>>> {
    let hash = name
        .bytes()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let index = cache.index(5)?;
    if let Some((&id, group)) = index.iter().find(|(_, g)| g.name_hash == Some(hash)) {
        Ok(Some(cache.file(
            5,
            id,
            *group.files.first().context("Empty terrain group")?,
        )?))
    } else {
        Ok(None)
    }
}
pub fn load(cache: &mut Cache, x: i32, y: i32) -> Result<Option<Region>> {
    let name = format!("m{x}_{y}");
    let Some(data) = named(cache, &name)? else {
        return Ok(None);
    };
    let mut region = Region::decode(&data, x, y).with_context(|| name)?;
    let name = format!("um{x}_{y}");
    if let Some(data) = named(cache, &name)? {
        region.underwater = Some(
            Region::decode_layer(&data, x, y, Some(&region.tiles))
                .with_context(|| name)?
                .tiles,
        );
    }
    Ok(Some(region))
}
pub struct World {
    pub regions: BTreeMap<(i32, i32), Region>,
    pub x: i32,
    pub y: i32,
}
impl World {
    pub fn load(cache: &mut Cache, x: i32, y: i32) -> Result<Self> {
        let mut regions = BTreeMap::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(r) = load(cache, x + dx, y + dy)? {
                    regions.insert((x + dx, y + dy), r);
                } else {
                    eprintln!(
                        "No neighboring terrain {}/{}; border samples clamped",
                        x + dx,
                        y + dy
                    );
                }
            }
        }
        ensure!(
            regions.contains_key(&(x, y)),
            "Region {x}/{y} is absent from cache"
        );
        Ok(Self { regions, x, y })
    }
    pub fn tile(&self, p: usize, x: i32, y: i32) -> Tile {
        let key = (self.x + x.div_euclid(64), self.y + y.div_euclid(64));
        if let Some(r) = self.regions.get(&key) {
            r.tile(p, x.rem_euclid(64) as usize, y.rem_euclid(64) as usize)
        } else {
            self.regions[&(self.x, self.y)].tile(
                p,
                x.clamp(0, 63) as usize,
                y.clamp(0, 63) as usize,
            )
        }
    }
    pub fn underwater_tile(&self, x: i32, y: i32) -> Option<Tile> {
        let key = (self.x + x.div_euclid(64), self.y + y.div_euclid(64));
        self.regions
            .get(&key)
            .and_then(|r| r.underwater.as_ref())
            .map(|tiles| tiles[x.rem_euclid(64) as usize * 64 + y.rem_euclid(64) as usize])
    }
    pub fn layer_tile(&self, underwater: bool, p: usize, x: i32, y: i32) -> Tile {
        if underwater {
            self.underwater_tile(x, y)
                .unwrap_or_else(|| self.tile(0, x, y))
        } else {
            self.tile(p, x, y)
        }
    }
    pub fn layer_normal(&self, underwater: bool, p: usize, x: i32, y: i32) -> [f32; 3] {
        let h = |x, y| -(self.layer_tile(underwater, p, x, y).height as f32);
        normalize([h(x - 1, y) - h(x + 1, y), h(x, y - 1) - h(x, y + 1), 256.])
    }
}
pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = v.iter().map(|a| a * a).sum::<f32>().sqrt();
    v.map(|a| a / l)
}
fn noise(x: i32, y: i32) -> i32 {
    let n = (x as u32).wrapping_add((y as u32).wrapping_mul(57));
    let h = n ^ (n << 13);
    let v = h
        .wrapping_mul(h)
        .wrapping_mul(15731)
        .wrapping_add(789221)
        .wrapping_mul(h)
        .wrapping_add(1376312589);
    ((v & 0x7fffffff) >> 19 & 255) as i32
}
fn smooth(x: i32, y: i32) -> i32 {
    (noise(x - 1, y - 1) + noise(x + 1, y - 1) + noise(x - 1, y + 1) + noise(x + 1, y + 1)) / 16
        + (noise(x - 1, y) + noise(x + 1, y) + noise(x, y - 1) + noise(x, y + 1)) / 8
        + noise(x, y) / 4
}
fn interp(a: i32, b: i32, t: i32, s: i32) -> i32 {
    let cos = ((t * 1024 / s) as f64 * std::f64::consts::PI / 1024.0).cos();
    let w = (65536 - (cos * 65536.0) as i32) >> 1;
    ((a * (65536 - w)) >> 16) + ((b * w) >> 16)
}
fn inoise(x: i32, y: i32, s: i32) -> i32 {
    let (a, b) = (x / s, y / s);
    interp(
        interp(smooth(a, b), smooth(a + 1, b), x & (s - 1), s),
        interp(smooth(a, b + 1), smooth(a + 1, b + 1), x & (s - 1), s),
        y & (s - 1),
        s,
    )
}
pub fn ground_height(x: i32, y: i32) -> i32 {
    let (x, y) = (x + 932731, y + 556238);
    let n = inoise(x + 45365, y + 91923, 4) - 128
        + ((inoise(x + 10294, y + 37821, 2) - 128) >> 1)
        + ((inoise(x, y, 1) - 128) >> 2);
    -(((n as f64 * 0.3) as i32 + 35).clamp(10, 60)) * 8
}
pub const VERTICES: [&[u8]; 13] = [
    &[1, 3, 5, 7],
    &[1, 3, 5, 7],
    &[1, 3, 5, 7],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 2, 6],
    &[1, 3, 5, 7, 2, 8],
    &[1, 3, 5, 7, 2, 8],
    &[1, 3, 5, 7, 11, 12],
    &[1, 3, 5, 7, 11, 12],
    &[1, 3, 5, 7, 13, 14],
];
pub const FACES: [&[usize]; 13] = [
    &[0, 1, 2, 3, 0, 0, 1, 3],
    &[1, 1, 2, 3, 1, 0, 1, 3],
    &[0, 1, 2, 3, 1, 0, 1, 3],
    &[0, 0, 1, 2, 0, 0, 2, 4, 1, 0, 4, 3],
    &[0, 0, 1, 4, 0, 0, 4, 3, 1, 1, 2, 4],
    &[0, 0, 4, 3, 1, 0, 1, 2, 1, 0, 2, 4],
    &[0, 1, 2, 4, 1, 0, 1, 4, 1, 0, 4, 3],
    &[0, 4, 1, 2, 0, 4, 2, 5, 1, 0, 4, 5, 1, 0, 5, 3],
    &[0, 4, 1, 2, 0, 4, 2, 3, 0, 4, 3, 5, 1, 0, 4, 5],
    &[0, 0, 4, 5, 1, 4, 1, 2, 1, 4, 2, 3, 1, 4, 3, 5],
    &[
        0, 0, 1, 5, 0, 1, 4, 5, 0, 1, 2, 4, 1, 0, 5, 3, 1, 5, 4, 3, 1, 4, 2, 3,
    ],
    &[
        1, 0, 1, 5, 1, 1, 4, 5, 1, 1, 2, 4, 0, 0, 5, 3, 0, 5, 4, 3, 0, 4, 2, 3,
    ],
    &[
        1, 0, 5, 4, 1, 0, 1, 5, 0, 0, 4, 3, 0, 4, 5, 3, 0, 5, 2, 3, 0, 1, 2, 5,
    ],
];
/// RT4 SceneGraph.OVERLAY_TRIANGLE_FLAGS selects the surface heightmap at
/// these unrotated vertices of an underwater underlay polygon. The remaining
/// vertices use the riverbed. Despite its name, the Java table is per vertex.
const SHORE_VERTICES: [&[[u8; 2]]; 12] = [
    &[],
    &[[128, 0], [0, 128]],
    &[[0, 0], [64, 128]],
    &[[128, 0], [64, 128]],
    &[[0, 0], [64, 128]],
    &[[64, 128], [128, 0]],
    &[[64, 0], [64, 128]],
    &[[0, 64], [64, 0]],
    &[[64, 0], [0, 64]],
    &[[0, 0], [128, 128], [64, 96], [32, 64]],
    &[[0, 0], [32, 64], [64, 96], [128, 128]],
    &[[0, 0], [32, 32], [96, 32], [128, 0]],
];
pub fn shoreline_vertex(shape: u8, rotation: u8, xy: [f32; 2]) -> bool {
    SHORE_VERTICES[shape as usize].iter().any(|&[x, y]| {
        let (x, y) = match rotation & 3 {
            1 => (y, 128 - x),
            2 => (128 - x, 128 - y),
            3 => (128 - y, x),
            _ => (x, y),
        };
        xy == [x as f32 / 128., y as f32 / 128.]
    })
}
/// Match RT4 SceneGraph.interpolateHeight, including the two integer shifts.
/// Interpolate in the original downward-positive coordinate system before
/// converting to Source Z, so negative rounding matches Java.
pub fn hd_height(heights: [i32; 4], xy: [f32; 2]) -> f32 {
    let x = (xy[0] * 128.) as i32;
    let y = (xy[1] * 128.) as i32;
    let south = (heights[0] * (128 - x) + heights[1] * x) >> 7;
    let north = (heights[3] * (128 - x) + heights[2] * x) >> 7;
    -((south * (128 - y) + north * y) >> 7) as f32
}
/// Position and height interpolation weights for the original shaped-tile vertices.
pub fn vertex(mut code: u8, rotation: u8) -> ([f32; 2], [f32; 4]) {
    if code <= 8 && code & 1 == 0 {
        code = ((code as i32 - rotation as i32 * 2 - 1) & 7) as u8 + 1;
    } else if (9..=12).contains(&code) {
        code = ((code as i32 - 9 - rotation as i32) & 3) as u8 + 9;
    } else if code >= 13 {
        code = ((code as i32 - 13 - rotation as i32) & 3) as u8 + 13;
    }
    let xy = [
        [0., 0.],
        [0.5, 0.],
        [1., 0.],
        [1., 0.5],
        [1., 1.],
        [0.5, 1.],
        [0., 1.],
        [0., 0.5],
        [0.5, 0.25],
        [0.75, 0.5],
        [0.5, 0.75],
        [0.25, 0.5],
        [0.25, 0.25],
        [0.75, 0.25],
        [0.75, 0.75],
        [0.25, 0.75],
    ][code as usize - 1];
    let [x, y] = xy;
    let w = [(1. - x) * (1. - y), x * (1. - y), x * y, (1. - x) * y];
    (xy, w)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn underwater_height_is_a_depth_from_the_surface_and_one_is_not_zero() {
        let mut surface = vec![Tile::default(); 16384];
        for t in &mut surface {
            t.height = -128;
        }
        let mut bytes = vec![];
        for _ in 0..4096 {
            bytes.extend_from_slice(&[1, 1]);
        }
        let result = Region::decode_layer(&bytes, 50, 50, Some(&surface)).unwrap();
        assert_eq!(result.tiles.len(), 4096);
        assert_eq!(result.tile(0, 0, 0).height, -120);
        assert_eq!(surface[0].height, -128);
    }
    #[test]
    fn hd_inset_height_uses_both_rows_and_java_rounding() {
        assert_eq!(hd_height([-8, -16, -40, -24], [0.25, 0.25]), 15.);
        assert_eq!(hd_height([-8, -16, -40, -24], [0., 0.]), 8.);
        assert_eq!(hd_height([-8, -16, -40, -24], [1., 1.]), 40.);
        // Java rounds negative intermediate results down rather than to zero.
        assert_eq!(hd_height([-1, 0, 0, 0], [0.5, 0.5]), 1.);
    }
    #[test]
    fn underwater_shoreline_flags_rotate_with_the_polygon() {
        // RT4 shape 6 has the two midpoint shoreline vertices. Both must use
        // surface heights while the outside corners remain on the riverbed.
        for rotation in 0..4 {
            let vertical = rotation % 2 == 0;
            for xy in [[0.5, 0.], [0.5, 1.], [0., 0.5], [1., 0.5]] {
                assert_eq!(
                    shoreline_vertex(6, rotation, xy),
                    (xy[0] == 0.5) == vertical
                );
            }
            for xy in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]] {
                assert!(!shoreline_vertex(6, rotation, xy));
            }
            assert!(!shoreline_vertex(0, rotation, [0., 0.]));
        }
        // Inset shoreline vertices in the final three shapes also use the
        // surface heightmap; these are not all on the outside tile edges.
        assert!(shoreline_vertex(9, 0, [0.25, 0.5]));
        assert!(shoreline_vertex(10, 0, [0.5, 0.75]));
        assert!(shoreline_vertex(11, 0, [0.75, 0.25]));
    }
    #[test]
    fn truncation_rejected() {
        assert!(Region::decode(&[0; 16], 50, 50).is_err());
    }
    #[test]
    fn upper_planes_relative() {
        let r = Region::decode(&vec![0; 16384], 50, 50).unwrap();
        assert_eq!(r.tile(1, 1, 1).height, r.tile(0, 1, 1).height - 240);
    }
    #[test]
    fn shapes_cover_tile() {
        for s in 0..13 {
            for rot in 0..4 {
                let mut area = 0.;
                for f in FACES[s].as_chunks::<4>().0 {
                    let v = f[1..]
                        .iter()
                        .map(|&i| {
                            let i = if i < 4 { (i + 4 - rot as usize) % 4 } else { i };
                            vertex(VERTICES[s][i], rot).0
                        })
                        .collect::<Vec<_>>();
                    area += ((v[1][0] - v[0][0]) * (v[2][1] - v[0][1])
                        - (v[1][1] - v[0][1]) * (v[2][0] - v[0][0]))
                        .abs()
                        / 2.;
                }
                assert!((area - 1.).abs() < 0.001, "shape {s} rotation {rot}");
            }
        }
    }
}
