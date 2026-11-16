// SPDX-License-Identifier: AGPL-3.0-only
//! RT4 ground blend masks, prepared offline. These are interpolation controls,
//! not rendered terrain tiles: native images and terrain triangles stay intact.
//! Reference: rt4/SceneGraph buildGlPlainTiles, addGlPlainTile and
//! buildGlShapedTiles (UNDERLAY_SHAPE_VERTICES/OVERLAY_SHAPE_VERTICES/edge masks).
use crate::{
    region::Floor,
    terrain::{Tile, World},
};
use anyhow::{ensure, Result};
use std::collections::{BTreeMap, BTreeSet};
pub const CONTROL_ID: i32 = -2;
const GRID: usize = 5 * 64 * 64;
const OVERLAY: [&[[u8; 2]]; 12] = [
    &[[0, 128], [0, 0], [128, 0], [128, 128]],
    &[[0, 128], [0, 0], [128, 0]],
    &[[0, 0], [64, 128], [0, 128]],
    &[[128, 128], [64, 128], [128, 0]],
    &[[0, 0], [128, 0], [128, 128], [64, 128]],
    &[[0, 128], [0, 0], [128, 0], [64, 128]],
    &[[64, 128], [0, 128], [0, 0], [64, 0]],
    &[[0, 0], [64, 0], [0, 64]],
    &[[128, 0], [128, 128], [0, 128], [0, 64], [64, 0]],
    &[[0, 128], [0, 0], [32, 64], [64, 96], [128, 128]],
    &[[0, 0], [128, 0], [128, 128], [64, 96], [32, 64]],
    &[[0, 0], [128, 0], [96, 32], [32, 32]],
];
const UNDERLAY: [&[[u8; 2]]; 12] = [
    &[],
    &[[128, 0], [128, 128], [0, 128]],
    &[[0, 0], [128, 0], [128, 128], [64, 128]],
    &[[0, 128], [0, 0], [128, 0], [64, 128]],
    &[[0, 0], [64, 128], [0, 128]],
    &[[128, 128], [64, 128], [128, 0]],
    &[[64, 0], [128, 0], [128, 128], [64, 128]],
    &[[128, 0], [128, 128], [0, 128], [0, 64], [64, 0]],
    &[[0, 0], [64, 0], [0, 64]],
    &[[0, 0], [128, 0], [128, 128], [64, 96], [32, 64]],
    &[[0, 128], [0, 0], [32, 64], [64, 96], [128, 128]],
    &[[0, 128], [0, 0], [32, 32], [96, 32], [128, 0], [128, 128]],
];
const EDGES: [usize; 48] = [
    1, 1, 1, 1, 4, 1, 1, 5, 6, 1, 5, 0, 7, 0, 4, 1, 7, 2, 1, 1, 6, 1, 1, 3, 6, 1, 7, 0, 0, 6, 7, 0,
    1, 7, 6, 1, 1, 1, 5, 4, 3, 2, 1, 1, 0, 4, 1, 5,
];
const MASKS: [&[bool]; 8] = [
    &[true, true, true],
    &[false, false],
    &[false, true],
    &[true, false],
    &[false, true, true],
    &[true, false, true],
    &[false, true, false],
    &[true, false, false],
];
// x, y, opacity (0 or 255, or RT4's bilinearly interpolated alpha).
pub type MaskTriangle = [[u8; 3]; 3];
#[derive(Clone, Debug, serde::Serialize)]
pub struct Layer {
    pub floor: u16,
    pub triangles: Vec<MaskTriangle>,
}
#[derive(Default, serde::Serialize)]
pub struct Blending {
    #[serde(skip)]
    pub tints: BTreeMap<u16, [f32; 4]>,
    pub rect: [usize; 4],
    pub plans: Vec<Vec<Layer>>,
    pub grid: Vec<usize>,
    #[serde(skip)]
    pub floors: BTreeMap<u16, Floor>,
}
fn rotate([x, y]: [u8; 2], r: u8) -> [u8; 2] {
    match r & 3 {
        1 => [y, 128 - x],
        2 => [128 - x, 128 - y],
        3 => [128 - y, x],
        _ => [x, y],
    }
}
fn fan(points: &[[u8; 3]]) -> Vec<MaskTriangle> {
    (1..points.len().saturating_sub(1))
        .map(|i| [points[0], points[i], points[i + 1]])
        .collect()
}
fn opaque(shape: u8, rotation: u8) -> Vec<MaskTriangle> {
    fan(&OVERLAY[shape as usize]
        .iter()
        .map(|&p| {
            let [x, y] = rotate(p, rotation);
            [x, y, 255]
        })
        .collect::<Vec<_>>())
}
fn extend(shape: u8, rotation: u8) -> Vec<MaskTriangle> {
    let v = OVERLAY[shape as usize];
    let a = |i: usize| [v[i][0], v[i][1], 255];
    let z = |x, y| [x, y, 0];
    let triangles = match shape {
        1 => vec![[z(128, 64), z(64, 128), a(2)], [z(64, 128), a(0), a(2)]],
        2 => vec![[a(0), z(64, 0), z(128, 128)], [z(128, 128), a(1), a(0)]],
        3 => vec![[a(2), a(1), z(0, 128)], [z(0, 128), z(64, 0), a(2)]],
        4 => vec![[a(3), z(0, 128), a(0)]],
        5 => vec![[a(2), z(128, 128), a(3)]],
        6 => vec![[a(3), z(128, 0), z(128, 128)], [z(128, 128), a(0), a(3)]],
        7 => vec![[a(1), z(128, 0), z(0, 128)], [z(0, 128), a(2), a(1)]],
        8 => vec![[a(3), z(0, 0), a(4)]],
        9 => vec![
            [z(96, 32), z(128, 64), a(4)],
            [z(96, 32), a(4), a(3)],
            [z(96, 32), a(3), a(2)],
            [z(96, 32), a(2), a(1)],
            [z(96, 32), a(1), z(64, 0)],
        ],
        10 => vec![
            [a(2), z(0, 128), a(3)],
            [a(3), z(0, 128), a(4)],
            [a(4), z(0, 128), a(0)],
        ],
        11 => vec![
            [a(3), z(0, 64), a(0)],
            [a(3), a(2), z(0, 64)],
            [a(2), z(128, 64), z(0, 64)],
            [a(2), a(1), z(128, 64)],
        ],
        _ => vec![],
    };
    triangles
        .into_iter()
        .map(|t| {
            t.map(|p| {
                let [x, y] = rotate([p[0], p[1]], rotation);
                [x, y, p[2]]
            })
        })
        .collect()
}
fn edge(t: Tile, side: usize) -> &'static [bool] {
    MASKS[EDGES[t.shape as usize * 4 + ((t.rotation as usize + side) & 3)]]
}
fn floor_key(f: &Floor) -> i64 {
    ((f.texture as i64 + 1) << 16) + f.scale as i64
}
fn alpha(x: u8, y: u8, c: [u8; 4]) -> u8 {
    let x = x as i32;
    let y = y as i32;
    let a = (c[0] as i32 * (128 - x) + c[1] as i32 * x) >> 7;
    let b = (c[3] as i32 * (128 - x) + c[2] as i32 * x) >> 7;
    ((a * (128 - y) + b * y) >> 7) as u8
}
fn plain_rotation(w: &World, uw: bool, p: usize, x: i32, y: i32, id: u8) -> u8 {
    let s = |dx, dy| {
        if w.layer_tile(uw, p, x + dx, y + dy).under == id {
            1i32
        } else {
            -1
        }
    };
    let sw = s(-1, -1) + s(0, -1) + s(-1, 0);
    let se = s(1, -1) + s(0, -1) + s(1, 0);
    let ne = s(1, 1) + s(1, 0) + s(0, 1);
    let nw = s(-1, 1) + s(0, 1) + s(-1, 0);
    if (se - nw).abs() <= (sw - ne).abs() {
        0
    } else {
        1
    }
}
impl Blending {
    pub fn build(world: &World, under: &BTreeMap<u8, Floor>, over: &BTreeMap<u8, Floor>) -> Self {
        let mut out = Self {
            plans: vec![vec![]],
            grid: vec![0; GRID],
            ..Self::default()
        };
        let mut dedup = BTreeMap::<Vec<u8>, usize>::new();
        for layer in 0..5 {
            let uw = layer == 4;
            let p = if uw { 0 } else { layer };
            for x in 0..64 {
                for y in 0..64 {
                    if uw && world.underwater_tile(x, y).is_none() {
                        continue;
                    }
                    let t = world.layer_tile(uw, p, x, y);
                    if t.under == 0 && t.over == 0 {
                        continue;
                    }
                    let mut layers = vec![];
                    let mut changed = false;
                    if t.under != 0 {
                        let corners =
                            [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)].map(|(cx, cy)| {
                                [(cx, cy), (cx - 1, cy), (cx, cy - 1), (cx - 1, cy - 1)]
                                    .into_iter()
                                    .map(|(a, b)| world.layer_tile(uw, p, a, b).under)
                                    .find(|&id| id != 0)
                                    .unwrap_or(t.under)
                            });
                        let keys = corners.map(|id| floor_key(&under[&id]));
                        let mut ids = corners.into_iter().collect::<Vec<_>>();
                        ids.sort_by_key(|id| floor_key(&under[id]));
                        ids.dedup_by_key(|id| floor_key(&under[id]));
                        changed = ids.len() > 1;
                        let poly = if t.over == 0 {
                            OVERLAY[0]
                        } else {
                            UNDERLAY[t.shape as usize]
                        };
                        let rotation = if t.over == 0 {
                            plain_rotation(world, uw, p, x, y, t.under)
                        } else {
                            t.rotation
                        };
                        for id in ids {
                            let key = floor_key(&under[&id]);
                            let c = keys.map(|k| if key <= k { 255 } else { 0 });
                            let points = poly
                                .iter()
                                .map(|&a| {
                                    let [a, b] = rotate(a, rotation);
                                    [a, b, alpha(a, b, c)]
                                })
                                .collect::<Vec<_>>();
                            let triangles = fan(&points);
                            if !triangles.is_empty() {
                                out.floors.insert(id as u16, under[&id].clone());
                                layers.push(Layer {
                                    floor: id as u16,
                                    triangles,
                                });
                            }
                        }
                    }
                    if !uw {
                        let mut overlays = BTreeMap::<u16, Vec<MaskTriangle>>::new();
                        if let Some(f) = over.get(&t.over).filter(|f| f.rgb != 0xff00ff) {
                            let mut mask = opaque(t.shape, t.rotation);
                            if f.blend && t.under != 0 {
                                let extra = extend(t.shape, t.rotation);
                                changed |= !extra.is_empty();
                                mask.extend(extra);
                            }
                            overlays.insert(256 + t.over as u16, mask);
                        }
                        if t.under != 0 || t.over != 0 && t.shape == 0 {
                            let mut neighbors = BTreeMap::new();
                            for (dx, dy) in [
                                (-1, -1),
                                (0, -1),
                                (1, -1),
                                (-1, 0),
                                (1, 0),
                                (-1, 1),
                                (0, 1),
                                (1, 1),
                            ] {
                                let n = world.tile(p, x + dx, y + dy);
                                if n.over == 0
                                    || n.over == t.over
                                    || !over[&n.over].blend
                                    || over[&n.over].rgb == 0xff00ff
                                {
                                    continue;
                                }
                                let visible = match (dx, dy) {
                                    (-1, -1) => !edge(n, 0)[1] || !edge(n, 3)[0],
                                    (1, -1) => !edge(n, 1)[1] || !edge(n, 0)[0],
                                    (1, 1) => !edge(n, 1)[1] || !edge(n, 2)[0],
                                    (-1, 1) => !edge(n, 3)[1] || !edge(n, 2)[0],
                                    _ => true,
                                };
                                if visible {
                                    neighbors.insert((dx, dy), n);
                                }
                            }
                            let ids = neighbors.values().map(|n| n.over).collect::<BTreeSet<_>>();
                            for id in ids {
                                let mask = |pos, side| {
                                    neighbors
                                        .get(&pos)
                                        .filter(|t| t.over == id)
                                        .map_or(MASKS[0], |t| edge(*t, side))
                                };
                                let north = mask((0, 1), 2);
                                let west = mask((-1, 0), 3);
                                let south = mask((0, -1), 0);
                                let east = mask((1, 0), 1);
                                let diagonal =
                                    |pos| neighbors.get(&pos).is_some_and(|n| n.over == id);
                                let val = |excluded| if excluded { 0 } else { 255 };
                                let nw = val(!diagonal((-1, 1)) && west[0] && north[1]);
                                let ne = val(!diagonal((1, 1)) && north[0] && east[1]);
                                let sw = val(!diagonal((-1, -1)) && south[0] && west[1]);
                                let se = val(!diagonal((1, -1)) && east[0] && south[1]);
                                let mut boundary = vec![[128, 128, ne]];
                                if north.len() > 2 {
                                    boundary.push([64, 128, val(north[2])]);
                                }
                                boundary.push([0, 128, nw]);
                                if west.len() > 2 {
                                    boundary.push([0, 64, val(west[2])]);
                                }
                                boundary.push([0, 0, sw]);
                                if south.len() > 2 {
                                    boundary.push([64, 0, val(south[2])]);
                                }
                                boundary.push([128, 0, se]);
                                if east.len() > 2 {
                                    boundary.push([128, 64, val(east[2])]);
                                }
                                boundary.push([128, 128, ne]);
                                let triangles = boundary
                                    .windows(2)
                                    .map(|v| [[64, 64, 0], v[0], v[1]])
                                    .filter(|t| t.iter().any(|p| p[2] != 0))
                                    .collect::<Vec<_>>();
                                if !triangles.is_empty() {
                                    changed = true;
                                    overlays
                                        .entry(256 + id as u16)
                                        .or_default()
                                        .extend(triangles);
                                }
                            }
                        }
                        let mut overlays = overlays.into_iter().collect::<Vec<_>>();
                        overlays.sort_by_key(|(id, _)| {
                            let f = &over[&((*id - 256) as u8)];
                            ((f.priority as i64) << 56)
                                + ((f.water_color as i64) << 32)
                                + floor_key(f)
                        });
                        for (id, triangles) in overlays {
                            out.floors.insert(id, over[&((id - 256) as u8)].clone());
                            layers.push(Layer {
                                floor: id,
                                triangles,
                            });
                        }
                    }
                    if changed {
                        let key = serde_json::to_vec(&layers).unwrap();
                        let index = *dedup.entry(key).or_insert_with(|| {
                            let i = out.plans.len();
                            out.plans.push(layers);
                            i
                        });
                        out.grid[(layer * 64 + x as usize) * 64 + y as usize] = index;
                    }
                }
            }
        }
        out
    }
    /// Encode compact triangle masks into the same atlas as native images.
    /// Each control is sampled exactly at mip 0; no mask rasterization is used.
    pub fn encode(
        &self,
        rects: &BTreeMap<i32, [usize; 4]>,
        textures: &crate::textures::Textures,
    ) -> Result<(usize, Vec<[u8; 4]>)> {
        let mut data = vec![[0; 4]; GRID];
        let mut descriptors = BTreeMap::new();
        let pair = |a: usize, b: usize| [a as u8, (a >> 8) as u8, b as u8, (b >> 8) as u8];
        for (&id, f) in &self.floors {
            descriptors.insert(id, data.len());
            let rect = rects.get(&f.texture).copied().unwrap_or([0; 4]);
            let tint = f.tint(textures.images.get(&f.texture));
            data.extend([
                pair(rect[0], rect[1]),
                pair(rect[2], rect[3]),
                pair(
                    (tint[0] * 8192.).round() as usize,
                    (tint[1] * 8192.).round() as usize,
                ),
                pair(
                    (tint[2] * 8192.).round() as usize,
                    (tint[3] * 8192.).round() as usize,
                ),
                pair(f.scale as usize, 0),
            ]);
        }
        let mut addresses = vec![0];
        for layers in self.plans.iter().skip(1) {
            addresses.push(data.len());
            ensure!(layers.len() <= 255, "Too many blend layers");
            data.push([layers.len() as u8, 0, 0, 0]);
            for l in layers {
                ensure!(l.triangles.len() <= 255, "Too many blend triangles");
                let addr = descriptors[&l.floor];
                data.push([
                    addr as u8,
                    (addr >> 8) as u8,
                    (addr >> 16) as u8,
                    l.triangles.len() as u8,
                ]);
                for t in &l.triangles {
                    for p in t {
                        data.push([p[0], p[1], p[2], 0]);
                    }
                }
            }
        }
        for (i, &plan) in self.grid.iter().enumerate() {
            let a = addresses[plan];
            data[i] = [a as u8, (a >> 8) as u8, (a >> 16) as u8, 0];
        }
        ensure!(
            data.len() < (1 << 24),
            "Blend control addresses exceed 24 bits"
        );
        let mut side = 2;
        while side * side < data.len() {
            side *= 2;
        }
        ensure!(side <= 8192, "Blend controls exceed atlas limit");
        data.resize(side * side, [0; 4]);
        Ok((side, data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::Region;
    fn floor(texture: i32, blend: bool) -> Floor {
        Floor {
            rgb: 0xffffff,
            texture,
            scale: 128.,
            blend,
            priority: 8,
            water_color: 1190717,
        }
    }
    fn world() -> World {
        let mut tiles = vec![Tile::default(); 16384];
        for t in &mut tiles[..4096] {
            t.under = 1;
        }
        World {
            regions: BTreeMap::from([(
                (50, 50),
                Region {
                    tiles,
                    underwater: None,
                },
            )]),
            x: 50,
            y: 50,
        }
    }
    fn opacity(tris: &[MaskTriangle], p: [f32; 2]) -> f32 {
        tris.iter()
            .filter_map(|t| {
                let a = t[0];
                let b = t[1];
                let c = t[2];
                let ab = [b[0] as f32 - a[0] as f32, b[1] as f32 - a[1] as f32];
                let ac = [c[0] as f32 - a[0] as f32, c[1] as f32 - a[1] as f32];
                let q = [p[0] - a[0] as f32, p[1] - a[1] as f32];
                let det = ab[0] * ac[1] - ab[1] * ac[0];
                if det.abs() < 0.5 {
                    return None;
                }
                let u = (q[0] * ac[1] - q[1] * ac[0]) / det;
                let v = (ab[0] * q[1] - ab[1] * q[0]) / det;
                if u.min(v).min(1. - u - v) < -0.00001 {
                    None
                } else {
                    Some((a[2] as f32 * (1. - u - v) + b[2] as f32 * u + c[2] as f32 * v) / 255.)
                }
            })
            .fold(0., f32::max)
    }
    #[test]
    fn uniform_ground_has_no_blend_work() {
        let w = world();
        let b = Blending::build(
            &w,
            &BTreeMap::from([(1, floor(1, false))]),
            &BTreeMap::new(),
        );
        assert!(b.grid.iter().all(|&i| i == 0));
        assert_eq!(b.plans.len(), 1);
    }
    #[test]
    fn underlay_transition_interpolates_shared_corner_texture_priorities() {
        let mut w = world();
        for x in 33..64 {
            for y in 0..64 {
                w.regions.get_mut(&(50, 50)).unwrap().tiles[x * 64 + y].under = 2;
            }
        }
        let b = Blending::build(
            &w,
            &BTreeMap::from([(1, floor(1, false)), (2, floor(2, false))]),
            &BTreeMap::new(),
        );
        let layers = &b.plans[b.grid[32 * 64 + 32]];
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[1].floor, 2);
        assert_eq!(opacity(&layers[1].triangles, [0., 64.]), 0.);
        assert!((opacity(&layers[1].triangles, [64., 64.]) - 0.5).abs() < 0.001);
        assert_eq!(opacity(&layers[1].triangles, [128., 64.]), 1.);
    }
    #[test]
    fn overlay_flag_enables_native_shape_extension_without_changing_mesh() {
        let mut w = world();
        let t = &mut w.regions.get_mut(&(50, 50)).unwrap().tiles[32 * 64 + 32];
        t.over = 1;
        t.shape = 1;
        let under = BTreeMap::from([(1, floor(1, false))]);
        let disabled = Blending::build(&w, &under, &BTreeMap::from([(1, floor(2, false))]));
        assert_eq!(disabled.grid[32 * 64 + 32], 0);
        let enabled = Blending::build(&w, &under, &BTreeMap::from([(1, floor(2, true))]));
        let l = enabled.plans[enabled.grid[32 * 64 + 32]]
            .iter()
            .find(|l| l.floor == 257)
            .unwrap();
        assert_eq!(opacity(&l.triangles, [64., 64.]), 1.);
        assert!((opacity(&l.triangles, [80., 80.]) - 0.5).abs() < 0.001);
        assert_eq!(opacity(&l.triangles, [96., 96.]), 0.);
        assert!(
            enabled.grid[31 * 64 + 32] != 0,
            "neighboring tile must receive an overlay edge mask"
        );
    }
    #[test]
    fn every_shape_extension_rotates_without_moving_mask_opacities() {
        for shape in 1..12 {
            for r in 0..4 {
                let a = extend(shape, 0);
                let b = extend(shape, r);
                assert!(!a.is_empty());
                for (a, b) in a.iter().zip(b.iter()) {
                    for (a, b) in a.iter().zip(b.iter()) {
                        assert_eq!(rotate([a[0], a[1]], r), [b[0], b[1]]);
                        assert_eq!(a[2], b[2]);
                    }
                }
            }
        }
    }
}
