// SPDX-License-Identifier: AGPL-3.0-only
//! Revision 530 tile decoding and shaped floor geometry. Shape tables adapted from
//! dennisdev/rs-map-viewer (BSD-2-Clause; see third_party/rs-map-viewer-LICENSE).
use crate::{binary::Reader, cache::Cache};
use anyhow::{bail, ensure, Context as _, Result};
use std::collections::BTreeMap;
/// Decoded native tile attributes on one map plane.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Tile {
    /// Native vertical coordinate, increasing downwards.
    pub height: i32,
    /// Underlay definition ID, with zero indicating none.
    pub under: u8,
    /// Overlay definition ID, with zero indicating none.
    pub over: u8,
    /// Native shaped-tile table index.
    pub shape: u8,
    /// Quarter-turn rotation of the native tile shape.
    pub rotation: u8,
    /// Native map flags for this tile.
    pub settings: u8,
}
/// Surface tiles and optional independently decoded underwater tiles.
pub(crate) struct Region {
    /// Four surface planes in plane/x/y order.
    pub tiles: Vec<Tile>,
    /// Optional underwater plane in x/y order.
    pub underwater: Option<Vec<Tile>>,
}
impl Region {
    /// Read a surface tile using native plane/x/y coordinates.
    #[allow(
        clippy::indexing_slicing,
        reason = "indices follow validated native tables and allocated image dimensions"
    )]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    pub(crate) fn tile(&self, p: usize, x: usize, y: usize) -> Tile {
        self.tiles[p * 4096 + x * 64 + y]
    }
    /// Decode a surface region at the given region coordinates.
    pub(crate) fn decode(bytes: &[u8], rx: i32, ry: i32) -> Result<Self> {
        Self::decode_layer(bytes, rx, ry, None)
    }
    /// Decode a surface or underwater group using native height rules.
    #[allow(
        clippy::indexing_slicing,
        reason = "indices follow validated native tables and allocated image dimensions"
    )]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::shadow_unrelated,
        reason = "each loop or conversion stage scopes its own intermediate values"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::integer_division,
        clippy::integer_division_remainder_used,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    fn decode_layer(bytes: &[u8], rx: i32, ry: i32, surface: Option<&[Tile]>) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let planes = if surface.is_some() { 1 } else { 4 };
        let mut tiles = vec![Tile::default(); planes * 4096];
        for p in 0..planes {
            for x in 0..64 {
                for y in 0..64 {
                    let i = p * 4096 + x * 64 + y;
                    let previous = surface.map_or_else(
                        || {
                            if p == 0 {
                                0_i32
                            } else {
                                tiles[i - 4096].height
                            }
                        },
                        |base| base[i].height,
                    );
                    let mut ended = false;
                    for _ in 0_i32..64_i32 {
                        let op = r.u8()?;
                        match op {
                            0 => {
                                tiles[i].height = if surface.is_some() {
                                    previous
                                } else if p == 0 {
                                    ground_height(rx * 64 + x as i32, ry * 64 + y as i32)
                                } else {
                                    previous - 240_i32
                                };
                                ended = true;
                                break;
                            }
                            1 => {
                                let h = r.u8()?;
                                tiles[i].height = if surface.is_some() {
                                    previous + i32::from(h) * 8_i32
                                } else {
                                    previous - if h == 1 { 0_i32 } else { i32::from(h) * 8_i32 }
                                };
                                ended = true;
                                break;
                            }
                            2..=49 => {
                                tiles[i].over = r.u8()?;
                                tiles[i].shape = (op - 2) / 4;
                                tiles[i].rotation = (op - 2) & 3;
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
                            let _consumed = r.take(len)?;
                        }
                    }
                }
                1 => {
                    let count = r.u8()?;
                    for _ in 0..count {
                        let _consumed = r.u8()?;
                        let _consumed = r.take(6)?;
                        let radius = r.u8()? as usize;
                        ensure!(radius <= 127, "Light radius too large");
                        let _consumed = r.take((2 * radius + 1) * 2)?;
                        let _consumed = r.u16()?;
                        let func = r.u8()?;
                        if func & 31 == 31 {
                            let _consumed = r.u16()?;
                        }
                    }
                }
                129 => {
                    for _ in 0_i32..4_i32 {
                        match r.u8()? {
                            0 | 2 => {}
                            1 => {
                                let _consumed = r.take(256)?;
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
/// Find and read a named map archive group.
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
fn named(cache: &mut Cache, name: &str) -> Result<Option<Vec<u8>>> {
    let hash = name
        .bytes()
        .fold(0_u32, |h, c| h.wrapping_mul(31).wrapping_add(u32::from(c)));
    let index = cache.index(5)?;
    if let Some((&id, group)) = index.iter().find(|&(_, g)| g.name_hash == Some(hash)) {
        Ok(Some(cache.file(
            5,
            id,
            *group.files.first().context("Empty terrain group")?,
        )?))
    } else {
        Ok(None)
    }
}
/// Read surface terrain and its optional underwater companion.
#[allow(
    clippy::shadow_unrelated,
    reason = "each loop or conversion stage scopes its own intermediate values"
)]
pub(crate) fn load(cache: &mut Cache, x: i32, y: i32) -> Result<Option<Region>> {
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
/// Requested region and neighboring terrain used at shared boundaries.
pub(crate) struct World {
    /// Available regions indexed by their native map coordinates.
    pub regions: BTreeMap<(i32, i32), Region>,
    /// Requested region X coordinate.
    pub x: i32,
    /// Requested region Y coordinate.
    pub y: i32,
}
impl World {
    /// Read surface terrain and its optional underwater companion.
    #[allow(
        clippy::print_stderr,
        reason = "the CLI reports errors and decoding progress to stderr"
    )]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    pub(crate) fn load(cache: &mut Cache, x: i32, y: i32) -> Result<Self> {
        let mut regions = BTreeMap::new();
        for dx in -1_i32..=1_i32 {
            for dy in -1_i32..=1_i32 {
                if let Some(r) = load(cache, x + dx, y + dy)? {
                    let _previous = regions.insert((x + dx, y + dy), r);
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
    /// Read a surface tile from the requested or neighboring region.
    #[allow(
        clippy::indexing_slicing,
        reason = "indices follow validated native tables and allocated image dimensions"
    )]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        clippy::cast_sign_loss,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    pub(crate) fn tile(&self, p: usize, x: i32, y: i32) -> Tile {
        let key = (self.x + x.div_euclid(64), self.y + y.div_euclid(64));
        self.regions.get(&key).map_or_else(
            || {
                self.regions[&(self.x, self.y)].tile(
                    p,
                    x.clamp(0_i32, 63_i32) as usize,
                    y.clamp(0_i32, 63_i32) as usize,
                )
            },
            |r| r.tile(p, x.rem_euclid(64) as usize, y.rem_euclid(64) as usize),
        )
    }
    /// Read an underwater tile, stitching shoreline heights to the surface.
    #[allow(
        clippy::indexing_slicing,
        reason = "indices follow validated native tables and allocated image dimensions"
    )]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    pub(crate) fn underwater_tile(&self, x: i32, y: i32) -> Option<Tile> {
        let key = (self.x + x.div_euclid(64), self.y + y.div_euclid(64));
        self.regions
            .get(&key)
            .and_then(|r| r.underwater.as_ref())
            .map(|tiles| tiles[x.rem_euclid(64) as usize * 64 + y.rem_euclid(64) as usize])
    }
    /// Select a tile from the surface or underwater plane.
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    pub(crate) fn layer_tile(&self, underwater: bool, p: usize, x: i32, y: i32) -> Tile {
        if underwater {
            self.underwater_tile(x, y)
                .unwrap_or_else(|| self.tile(0, x, y))
        } else {
            self.tile(p, x, y)
        }
    }
    /// Calculate a native height-derived terrain normal.
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::shadow_unrelated,
        reason = "each loop or conversion stage scopes its own intermediate values"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        clippy::cast_precision_loss,
        clippy::float_arithmetic,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    pub(crate) fn layer_normal(&self, underwater: bool, p: usize, x: i32, y: i32) -> [f32; 3] {
        let h = |x, y| -(self.layer_tile(underwater, p, x, y).height as f32);
        normalize([h(x - 1, y) - h(x + 1, y), h(x, y - 1) - h(x, y + 1), 256.])
    }
}
/// Normalize a three-component direction.
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::float_arithmetic,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
pub(crate) fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = v.iter().map(|a| a * a).sum::<f32>().sqrt();
    v.map(|a| a / l)
}
/// Evaluate the native wrapping integer height hash.
#[allow(
    clippy::many_single_char_names,
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
const fn noise(x: i32, y: i32) -> i32 {
    let n = (x as u32).wrapping_add((y as u32).wrapping_mul(57));
    let h = n ^ (n << 13_i32);
    let v = h
        .wrapping_mul(h)
        .wrapping_mul(15731)
        .wrapping_add(789_221)
        .wrapping_mul(h)
        .wrapping_add(1_376_312_589);
    (((v & 0x7fff_ffff) >> 19) & 255) as i32
}
/// Smooth the native height hash using neighboring grid points.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
const fn smooth(x: i32, y: i32) -> i32 {
    (noise(x - 1, y - 1) + noise(x + 1, y - 1) + noise(x - 1, y + 1) + noise(x + 1, y + 1)) / 16
        + (noise(x - 1, y) + noise(x + 1, y) + noise(x, y - 1) + noise(x, y + 1)) / 8
        + noise(x, y) / 4
}
/// Interpolate native height samples using the cosine table rule.
#[allow(
    clippy::many_single_char_names,
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::float_arithmetic,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn interp(a: i32, b: i32, t: i32, s: i32) -> i32 {
    let cos = (f64::from(t * 1024_i32 / s) * core::f64::consts::PI / 1024.0_f64).cos();
    let w = (65_536_i32 - (cos * 65_536.0_f64) as i32) >> 1_i32;
    ((a * (65536 - w)) >> 16) + ((b * w) >> 16)
}
/// Sample smoothed height noise at the requested grid scale.
#[allow(
    clippy::many_single_char_names,
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn inoise(x: i32, y: i32, s: i32) -> i32 {
    let (a, b) = (x / s, y / s);
    interp(
        interp(smooth(a, b), smooth(a + 1, b), x & (s - 1), s),
        interp(smooth(a, b + 1), smooth(a + 1, b + 1), x & (s - 1), s),
        y & (s - 1),
        s,
    )
}
/// Generate the native base-plane height when no explicit height is stored.
#[allow(
    clippy::shadow_reuse,
    reason = "local rebinding records successive numeric representations"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::float_arithmetic,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
pub(crate) fn ground_height(x: i32, y: i32) -> i32 {
    let (x, y) = (x + 932_731_i32, y + 556_238_i32);
    let n = inoise(x + 45365, y + 91923, 4) - 128_i32
        + ((inoise(x + 10294, y + 37821, 2) - 128_i32) >> 1_i32)
        + ((inoise(x, y, 1) - 128_i32) >> 2_i32);
    -(((f64::from(n) * 0.3) as i32 + 35).clamp(10, 60)) * 8
}
/// Native shaped-tile vertex lookup table.
pub(crate) const VERTICES: [&[u8]; 13] = [
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
/// Native shaped-tile triangle lookup table.
pub(crate) const FACES: [&[usize]; 13] = [
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
/// RT4 `SceneGraph.OVERLAY_TRIANGLE_FLAGS` selects the surface heightmap at
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
/// Determine whether a shaped-tile vertex joins the surface shoreline.
#[allow(
    clippy::indexing_slicing,
    reason = "indices follow validated native tables and allocated image dimensions"
)]
#[allow(
    clippy::float_cmp,
    reason = "exact native vertex and material identity; approximate equality would merge distinct data"
)]
#[allow(
    clippy::shadow_reuse,
    reason = "local rebinding records successive numeric representations"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::float_arithmetic,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
pub(crate) fn shoreline_vertex(shape: u8, rotation: u8, xy: [f32; 2]) -> bool {
    SHORE_VERTICES[shape as usize].iter().any(|&[x, y]| {
        let (x, y) = match rotation & 3 {
            1 => (y, 128 - x),
            2 => (128 - x, 128 - y),
            3 => (128 - y, x),
            _ => (x, y),
        };
        xy == [f32::from(x) / 128., f32::from(y) / 128.]
    })
}
/// Match RT4 SceneGraph.interpolateHeight, including the two integer shifts.
/// Interpolate in the original downward-positive coordinate system before
/// converting to Source Z, so negative rounding matches Java.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::float_arithmetic,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
pub(crate) fn hd_height(heights: [i32; 4], xy: [f32; 2]) -> f32 {
    let x = (xy[0] * 128.) as i32;
    let y = (xy[1] * 128.) as i32;
    let south = (heights[0] * (128_i32 - x) + heights[1] * x) >> 7_i32;
    let north = (heights[3] * (128_i32 - x) + heights[2] * x) >> 7_i32;
    -((south * (128 - y) + north * y) >> 7) as f32
}
/// Position and height interpolation weights for the original shaped-tile vertices.
#[allow(
    clippy::indexing_slicing,
    reason = "indices follow validated native tables and allocated image dimensions"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_sign_loss,
    clippy::float_arithmetic,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
pub(crate) fn vertex(mut code: u8, rotation: u8) -> ([f32; 2], [f32; 4]) {
    if code <= 8 && code & 1 == 0 {
        code = ((i32::from(code) - i32::from(rotation) * 2_i32 - 1_i32) & 7_i32) as u8 + 1;
    } else if (9..=12).contains(&code) {
        code = ((i32::from(code) - 9_i32 - i32::from(rotation)) & 3_i32) as u8 + 9;
    } else if code >= 13 {
        code = ((i32::from(code) - 13_i32 - i32::from(rotation)) & 3_i32) as u8 + 13;
    } else {
        // Odd perimeter vertex codes keep their original corner position.
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
#[allow(
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "test fixtures use exact values, fixed indexes and panic on unexpected failures"
)]
mod tests {
    use super::*;
    #[test]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    fn underwater_height_is_a_depth_from_the_surface_and_one_is_not_zero() {
        let mut surface = vec![Tile::default(); 16384];
        for t in &mut surface {
            t.height = -128_i32;
        }
        let mut bytes = vec![];
        for _ in 0_i32..4_096_i32 {
            bytes.extend_from_slice(&[1, 1]);
        }
        let result = Region::decode_layer(&bytes, 50, 50, Some(&surface)).unwrap();
        assert_eq!(result.tiles.len(), 4096);
        assert_eq!(result.tile(0, 0, 0).height, -120_i32);
        assert_eq!(surface[0].height, -128_i32);
    }
    #[test]
    fn hd_inset_height_uses_both_rows_and_java_rounding() {
        assert_eq!(
            hd_height([-8_i32, -16_i32, -40_i32, -24_i32], [0.25, 0.25]),
            15.
        );
        assert_eq!(hd_height([-8_i32, -16_i32, -40_i32, -24_i32], [0., 0.]), 8.);
        assert_eq!(
            hd_height([-8_i32, -16_i32, -40_i32, -24_i32], [1., 1.]),
            40.
        );
        // Java rounds negative intermediate results down rather than to zero.
        assert_eq!(hd_height([-1_i32, 0_i32, 0_i32, 0_i32], [0.5, 0.5]), 1.);
    }
    #[test]
    #[allow(
        clippy::integer_division_remainder_used,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
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
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    fn upper_planes_relative() {
        let r = Region::decode(&vec![0; 16384], 50, 50).unwrap();
        assert_eq!(r.tile(1, 1, 1).height, r.tile(0, 1, 1).height - 240_i32);
    }
    #[test]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::shadow_reuse,
        reason = "local rebinding records successive numeric representations"
    )]
    #[allow(
        clippy::as_conversions,
        clippy::integer_division_remainder_used,
        clippy::suboptimal_flops,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
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
