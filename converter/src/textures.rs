// SPDX-License-Identifier: AGPL-3.0-or-later
//! Offline revision-530 procedural textures. Fixed-point operations adapted from
//! rs-map-viewer (BSD-2-Clause; see third_party/rs-map-viewer-LICENSE).
use crate::{binary::Reader, cache::Cache};
use anyhow::{bail, ensure, Context as _, Result};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
/// Native fixed-point RGB texture pixels.
#[allow(
    unmarked_unchecked_type_aliases,
    reason = "concrete alias has no generic bounds"
)]
type Image = Vec<[i32; 3]>;
/// One procedural texture graph node.
#[derive(Clone)]
struct Op {
    /// Native procedural operation opcode.
    kind: u8,
    /// Operation parameters indexed by native field opcode.
    fields: BTreeMap<u8, Vec<i32>>,
    /// Indices of upstream graph nodes.
    inputs: Vec<usize>,
    /// Whether the operation produces a monochrome output.
    mono: bool,
}
impl Op {
    /// Read a scalar operation parameter, falling back to its native default.
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    fn v(&self, id: u8, default: i32) -> i32 {
        self.fields
            .get(&id)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(default)
    }
}
/// Generated native texture and metadata used by floor shading.
#[derive(Clone)]
#[allow(
    clippy::partial_pub_fields,
    reason = "expose render metadata while encapsulating internal caches"
)]
pub(crate) struct Texture {
    /// Unscaled native RGBA pixels.
    pub pixels: Vec<[u8; 4]>,
    /// Native square image dimension.
    pub size: usize,
    /// Native special-effect/material classification.
    pub effect: u8,
    /// Native floor-color brightness interpolation factor.
    pub brightness: u8,
    /// Native floor-color brightness boost.
    pub boost: u8,
    /// Fixed-point RGB pixels for dependent procedural textures.
    dependency: Image,
}
/// Procedural texture cache and revision-530 metadata.
#[allow(
    clippy::partial_pub_fields,
    reason = "expose render metadata while encapsulating internal caches"
)]
pub(crate) struct Textures {
    /// Generated textures keyed by their cache IDs.
    pub images: BTreeMap<i32, Texture>,
    /// Special-effect classifications indexed by texture ID.
    effects: Vec<u8>,
    /// Native image dimensions indexed by texture ID.
    sizes: Vec<usize>,
    /// Native floor-color brightness interpolation factor.
    brightness: Vec<u8>,
    /// Native floor-color brightness boost.
    boost: Vec<u8>,
    /// Texture IDs currently being generated, for cycle detection.
    active: HashSet<i32>,
}
#[cfg(test)]
impl Textures {
    pub(crate) fn fixture(images: BTreeMap<i32, (usize, Vec<[u8; 4]>)>) -> Self {
        Self {
            images: images
                .into_iter()
                .map(|(id, (size, pixels))| {
                    (
                        id,
                        Texture {
                            pixels,
                            size,
                            effect: 0,
                            brightness: 0,
                            boost: 0,
                            dependency: vec![],
                        },
                    )
                })
                .collect(),
            effects: vec![],
            sizes: vec![],
            brightness: vec![],
            boost: vec![],
            active: HashSet::new(),
        }
    }
}
impl Textures {
    /// Read texture metadata from the native cache.
    #[allow(
        clippy::indexing_slicing,
        reason = "indices follow validated native tables and allocated image dimensions"
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
        clippy::as_conversions,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    pub(crate) fn new(cache: &mut Cache) -> Result<Self> {
        let bytes = cache.file(26, 0, 0)?;
        let mut r = Reader::new(&bytes);
        let count = r.u16()? as usize;
        let exists = core::iter::repeat_with(|| r.u8().map(|b| b == 1))
            .take(count)
            .collect::<Result<Vec<_>>>()?;
        let mut effects = vec![0; count];
        let mut sizes = vec![128; count];
        let mut brightness = vec![0; count];
        let mut boost = vec![0; count];
        for field in 0_i32..9_i32 {
            for i in 0..count {
                if !exists[i] {
                    continue;
                }
                if field == 8_i32 {
                    let _consumed = r.u16()?;
                } else {
                    let b = r.u8()?;
                    match field {
                        2_i32 => sizes[i] = if b == 1 { 64 } else { 128 },
                        6_i32 => effects[i] = b,
                        5_i32 => brightness[i] = b,
                        4_i32 => boost[i] = b,
                        _ => {}
                    }
                }
            }
        }
        r.finish()?;
        Ok(Self {
            images: BTreeMap::new(),
            effects,
            sizes,
            brightness,
            boost,
            active: HashSet::new(),
        })
    }
    /// Generate a missing texture, detecting recursive dependencies.
    pub(crate) fn load(&mut self, cache: &mut Cache, id: i32) -> Result<()> {
        if self.images.contains_key(&id) {
            return Ok(());
        }
        ensure!(self.active.insert(id), "Cyclic texture dependency {id}");
        let result = self.generate(cache, id);
        let _consumed = self.active.remove(&id);
        let image = result.with_context(|| format!("Procedural texture {id}"))?;
        let _previous = self.images.insert(id, image);
        Ok(())
    }
    /// Decode and evaluate one native procedural texture graph.
    #[allow(
        clippy::too_many_lines,
        reason = "keep native opcode dispatch and ordered conversion stages together"
    )]
    #[allow(
        clippy::indexing_slicing,
        reason = "indices follow validated native tables and allocated image dimensions"
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
        clippy::shadow_unrelated,
        reason = "each loop or conversion stage scopes its own intermediate values"
    )]
    #[allow(
        clippy::absolute_paths,
        reason = "qualified names identify the format or IO operation directly"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::float_arithmetic,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    fn generate(&mut self, cache: &mut Cache, id: i32) -> Result<Texture> {
        let n = *self
            .sizes
            .get(id as usize)
            .context("Texture metadata missing")?;
        let data = cache.file(9, id as u32, 0)?;
        let mut r = Reader::new(&data);
        let count = r.u8()? as usize;
        ensure!(count > 0, "Empty texture graph");
        let mut ops = vec![];
        for _ in 0..count {
            let _consumed = r.u8()?;
            let kind = r.u8()?;
            let _consumed = r.u8()?;
            let ct = r.u8()?;
            let mut fields = BTreeMap::new();
            for _ in 0..ct {
                let f = r.u8()?;
                let values = match (kind, f) {
                    (8, 0) => {
                        let mode = i32::from(r.u8()?);
                        let count = r.u8()? as usize;
                        let mut v = vec![mode];
                        for _ in 0..count {
                            v.push(i32::from(r.u16()?));
                            v.push(i32::from(r.u16()?));
                        }
                        v
                    }
                    (10, 0) => {
                        let preset = r.u8()?;
                        ensure!(preset == 0, "Gradient preset {preset} not yet supported");
                        let count = r.u8()?;
                        let mut v = vec![];
                        for _ in 0..count {
                            v.push(i32::from(r.u16()?));
                            for _ in 0_i32..3_i32 {
                                v.push(i32::from(r.u8()?) << 4_i32);
                            }
                        }
                        v
                    }
                    (34, 2) => {
                        let persistence = i32::from(r.u16()? as i16);
                        let mut v = vec![persistence];
                        if persistence < 0_i32 {
                            let oct = fields
                                .get(&1)
                                .and_then(|v: &Vec<i32>| v.first())
                                .copied()
                                .unwrap_or(4_i32);
                            for _ in 0_i32..oct {
                                v.push(i32::from(r.u16()? as i16));
                            }
                        }
                        v
                    }
                    (1, 0) => vec![r.u24()? as i32],
                    (5 | 9, 0..=2)
                    | (0 | 21 | 22 | 23 | 38, 0)
                    | (4 | 7 | 20 | 34 | 15, 0..=1)
                    | (6 | 30 | 38, 2)
                    | (19, 1)
                    | (34 | 15, 3..=6) => vec![i32::from(r.u8()?)],
                    (4, 2..=7)
                    | (6 | 30, 0..=1)
                    | (15, 2)
                    | (19 | 36 | 39 | 18, 0)
                    | (32, 0..=2)
                    | (38, 1 | 3..=4) => vec![i32::from(r.u16()?)],
                    _ => bail!("Unsupported texture operation {kind} field {f}"),
                };
                let _previous = fields.insert(f, values);
            }
            let input_count = match kind {
                0..=4 | 13 | 15 | 18 | 34 | 36 | 38 | 39 => 0_usize,
                7 => 2_usize,
                19 | 21 => 3_usize,
                5 | 6 | 8..=10 | 20 | 22..=24 | 30 | 32 => 1_usize,
                _ => bail!("Unsupported texture operation {kind}"),
            };
            let inputs = core::iter::repeat_with(|| r.u8().map(|v| v as usize))
                .take(input_count)
                .collect::<Result<Vec<_>>>()?;
            let mono = match kind {
                0 | 2 | 3 | 4 | 8 | 13 | 15 | 32 | 34 | 38 => true,
                6 => fields.get(&2).is_some_and(|v| v[0] == 1_i32),
                7 => fields.get(&1).is_some_and(|v| v[0] == 1_i32),
                19 => fields.get(&1).is_some_and(|v| v[0] == 1_i32),
                21 => fields.get(&0).is_some_and(|v| v[0] == 1_i32),
                30 => fields.get(&2).is_some_and(|v| v[0] == 1_i32),
                _ => false,
            };
            ops.push(Op {
                kind,
                fields,
                inputs,
                mono,
            });
        }
        let colour = r.u8()? as usize;
        let alpha = r.u8()? as usize;
        ensure!(colour < count && alpha < count, "Invalid texture outputs");
        // Revision 530 appends sampler/animation flags after the two graph roots.
        let mut results = HashMap::new();
        let mut visiting = HashSet::new();
        let c = self.eval(cache, &ops, colour, &mut results, &mut visiting, n)?;
        let a = self.eval(cache, &ops, alpha, &mut results, &mut visiting, n)?;
        let _blurred = r.u8()? == 1;
        let flip = r.u8()? == 1;
        let _consumed = r.take(7)?;
        r.finish()?;
        let mut dependency = vec![[0_i32; 3]; n * n];
        for y in 0..n {
            for x in 0..n {
                let dst = if flip { x * n + y } else { y * n + x };
                dependency[dst] = c[y * n + x].map(|v| (v >> 4_i32).clamp(0_i32, 255_i32) * 16_i32);
            }
        }
        let mut pixels = vec![[0; 4]; n * n];
        for y in 0..n {
            for x in 0..n {
                let src = y * n + (n - 1 - x);
                let dst = if flip { x * n + y } else { y * n + x };
                for channel in 0..3 {
                    pixels[dst][channel] =
                        ((f64::from((c[src][channel] >> 4_i32).clamp(0_i32, 255_i32)) / 255.)
                            .powf(0.7)
                            * 255.0_f64) as u8;
                }
                pixels[dst][3] = if pixels[dst][..3] == [0, 0, 0] {
                    0
                } else {
                    (a[src][0] >> 4_i32).clamp(0_i32, 255_i32) as u8
                };
            }
        }
        eprintln!("Decoded procedural texture {id}: {} operations", ops.len());
        Ok(Texture {
            pixels,
            size: n,
            dependency,
            effect: self.effects.get(id as usize).copied().unwrap_or(0),
            brightness: self.brightness.get(id as usize).copied().unwrap_or(0),
            boost: self.boost.get(id as usize).copied().unwrap_or(0),
        })
    }
    /// Evaluate one graph node using native fixed-point arithmetic.
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
        clippy::modulo_arithmetic,
        reason = "Java signed remainder is part of the native procedural algorithm"
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
        clippy::absolute_paths,
        reason = "qualified names identify the format or IO operation directly"
    )]
    #[allow(
        clippy::shadow_reuse,
        reason = "local rebinding records successive numeric representations"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::float_arithmetic,
        clippy::integer_division,
        clippy::integer_division_remainder_used,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    fn eval(
        &mut self,
        cache: &mut Cache,
        ops: &[Op],
        index: usize,
        results: &mut HashMap<usize, Arc<Image>>,
        visiting: &mut HashSet<usize>,
        n: usize,
    ) -> Result<Arc<Image>> {
        if let Some(img) = results.get(&index) {
            return Ok(Arc::clone(img));
        }
        ensure!(visiting.insert(index), "Cyclic texture graph");
        let op = ops.get(index).context("Invalid texture input")?;
        let mut input = vec![];
        for &i in &op.inputs {
            input.push(self.eval(cache, ops, i, results, visiting, n)?);
        }
        let mut out = vec![[0_i32; 3]; n * n];
        let scalar = |v: i32| [v; 3];
        match op.kind {
            0 => out.fill(scalar(op.v(0, 255) * 4_096_i32 / 255_i32)),
            1 => {
                let rgb = op.v(0, 0);
                out.fill([
                    ((rgb >> 16_i32) & 0xff_i32) * 16_i32,
                    ((rgb >> 8_i32) & 0xff_i32) * 16_i32,
                    (rgb & 0xff_i32) * 16_i32,
                ]);
            }
            2 | 3 => {
                for y in 0..n {
                    for x in 0..n {
                        out[y * n + x] = scalar(
                            (if op.kind == 2 { x } else { y }) as i32 * 4_096_i32 / n as i32,
                        );
                    }
                }
            }
            4 => bricks(op, &mut out, n)?,
            5 => {
                let hx = op.v(0, 1);
                let hy = op.v(1, 1);
                ensure!(
                    (0_i32..=32_i32).contains(&hx) && (0_i32..=32_i32).contains(&hy),
                    "Blur too large"
                );
                let mut pass = vec![[0_i32; 3]; n * n];
                let ix = 65_536_i32 / (2_i32 * hx + 1_i32);
                let iy = 65_536_i32 / (2_i32 * hy + 1_i32);
                for y in 0..n {
                    for x in 0..n {
                        for c in 0..3 {
                            let sum = (-hx..=hx)
                                .map(|dx| input[0][y * n + wrap(x as i32 + dx, n)][c])
                                .sum::<i32>();
                            pass[y * n + x][c] = sum * ix / 65_536_i32;
                        }
                    }
                }
                for y in 0..n {
                    for x in 0..n {
                        for c in 0..3 {
                            out[y * n + x][c] = (-hy..=hy)
                                .map(|dy| pass[wrap(y as i32 + dy, n) * n + x][c])
                                .sum::<i32>()
                                * iy
                                / 65_536_i32;
                        }
                    }
                }
            }
            6 => {
                let lo = op.v(0, 0);
                let hi = op.v(1, 4096);
                ensure!(lo <= hi, "Invalid clamp range");
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = i.map(|v| v.clamp(lo, hi));
                }
            }
            7 => {
                for (i, o) in out.iter_mut().enumerate() {
                    for (c, v) in o.iter_mut().enumerate() {
                        let (a, b) = (input[0][i][c], input[1][i][c]);
                        *v = match op.v(0, 6) {
                            1_i32 => a + b,
                            2_i32 => a - b,
                            3_i32 => a * b / 4_096_i32,
                            4_i32 => {
                                if b == 0_i32 {
                                    4_096_i32
                                } else {
                                    a * 4_096_i32 / b
                                }
                            }
                            5_i32 => 4_096_i32 - (4_096_i32 - a) * (4_096_i32 - b) / 4_096_i32,
                            6_i32 => {
                                if b < 2_048_i32 {
                                    b * a / 2_048_i32
                                } else {
                                    4_096_i32 - (4_096_i32 - a) * (4_096_i32 - b) / 2_048_i32
                                }
                            }
                            7_i32 => {
                                if a == 4_096_i32 {
                                    4_096_i32
                                } else {
                                    b * 4_096_i32 / (4_096_i32 - a)
                                }
                            }
                            8_i32 => {
                                if a == 0_i32 {
                                    0_i32
                                } else {
                                    4_096_i32 - (4_096_i32 - b) * 4_096_i32 / a
                                }
                            }
                            9_i32 => a.min(b),
                            10_i32 => a.max(b),
                            11_i32 => (a - b).abs(),
                            12_i32 => a + b - a * b / 2_048_i32,
                            n => bail!("Unknown arithmetic mode {n}"),
                        };
                    }
                }
            }
            8 => {
                let table = curve(op)?;
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = scalar(table[(i[0] / 16_i32).clamp(0_i32, 256_i32) as usize]);
                }
            }
            9 => {
                for y in 0..n {
                    for x in 0..n {
                        let xx = if op.v(0, 1) == 1_i32 { n - 1 - x } else { x };
                        let yy = if op.v(1, 1) == 1_i32 { n - 1 - y } else { y };
                        out[y * n + x] = input[0][yy * n + xx];
                    }
                }
            }
            10 => {
                let markers = op.fields.get(&0).context("Missing colour gradient")?;
                ensure!(markers.len() >= 4, "Gradient needs two markers");
                let table = gradient(markers)?;
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = table[(i[0] >> 4_i32).clamp(0_i32, 256_i32) as usize];
                }
            }
            13 => {
                for y in 0..n {
                    for x in 0..n {
                        let hash_input = (x as i32 * 4_096_i32 / n as i32)
                            + (y as i32 * 4_096_i32 / n as i32) * 57_i32;
                        let h = hash_input ^ (hash_input << 1_i32);
                        let v = h
                            .wrapping_mul(h)
                            .wrapping_mul(15731)
                            .wrapping_add(789_221)
                            .wrapping_mul(h)
                            .wrapping_add(1_376_312_589)
                            & 0x7fff_ffff_i32;
                        out[y * n + x] = scalar((4_096_i32 - v / 262_144_i32) % 4_096_i32);
                    }
                }
            }
            15 => voronoi(op, &mut out, n)?,
            19 => {
                for y in 0..n {
                    for x in 0..n {
                        let i = y * n + x;
                        let a = (input[1][i][0] >> 4_i32) & 0xff_i32;
                        let angle = f64::from(a) / 255.0_f64 * core::f64::consts::TAU;
                        let amp = (input[2][i][0] * op.v(0, 32768) * 16_i32) >> 12_i32;
                        let xx = x as i32
                            + ((((angle.cos() * 4_096.0_f64) as i32 * amp) >> 12_i32) >> 12_i32);
                        let yy = y as i32
                            + ((((angle.sin() * 4_096.0_f64) as i32 * amp) >> 12_i32) >> 12_i32);
                        out[i] = input[0][wrap(yy, n) * n + wrap(xx, n)];
                    }
                }
            }
            20 => {
                let (h, v) = (op.v(0, 4), op.v(1, 4));
                ensure!(
                    h > 0_i32 && v > 0_i32 && h <= n as i32 && v <= n as i32,
                    "Invalid tile count"
                );
                for y in 0..n {
                    for x in 0..n {
                        let xx = (x % (n / h as usize)) * h as usize;
                        let yy = (y % (n / v as usize)) * v as usize;
                        out[y * n + x] = input[0][yy * n + xx];
                    }
                }
            }
            21 => {
                for (i, o) in out.iter_mut().enumerate() {
                    let w = input[2][i][0];
                    for (c, v) in o.iter_mut().enumerate() {
                        *v = (input[0][i][c] * w + input[1][i][c] * (4_096_i32 - w)) >> 12_i32;
                    }
                }
            }
            22 => {
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = i.map(|v| 4_096_i32 - v);
                }
            }
            24 => {
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = scalar((i[0] + i[1] + i[2]) / 3_i32);
                }
            }
            30 => {
                let a = op.v(0, 1024);
                let b = op.v(1, 3072);
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = i.map(|v| a + ((v * (b - a)) >> 12_i32));
                }
            }
            32 => emboss(op, &input[0], &mut out, n),
            34 => perlin(op, &mut out, n)?,
            36 => {
                let id = op.v(0, -1);
                ensure!(id >= 0_i32, "Missing texture dependency");
                self.load(cache, id)?;
                let dep = &self.images[&id];
                for y in 0..n {
                    for x in 0..n {
                        out[y * n + x] =
                            dep.dependency[(y * dep.size / n) * dep.size + x * dep.size / n];
                    }
                }
            }
            38 => lines(op, &mut out, n)?,
            18 | 39 => bail!(
                "Sprite textures require the sprite decoder (operation {})",
                op.kind
            ),
            n => bail!("Unsupported texture operation {n}"),
        }
        if op.mono {
            for p in &mut out {
                *p = scalar(p[0]);
            }
        }
        let _consumed = visiting.remove(&index);
        let out = Arc::new(out);
        let _previous = results.insert(index, Arc::clone(&out));
        Ok(out)
    }
}
/// Wrap a signed pixel coordinate into the native image dimension.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_sign_loss,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
const fn wrap(x: i32, n: usize) -> usize {
    (x as usize) & (n - 1)
}
/// Java-compatible 48-bit random number generator.
struct Random(u64);
impl Random {
    /// Seed the Java-compatible random generator.
    #[allow(
        clippy::as_conversions,
        clippy::cast_sign_loss,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    const fn new(seed: i32) -> Self {
        Self((seed as u64 ^ 0x0005_deec_e66d) & ((1 << 48) - 1))
    }
    /// Advance the Java random state and return its high 32 bits.
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    const fn bits(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x0005_deec_e66d).wrapping_add(11) & ((1 << 48_i32) - 1);
        (self.0 >> 16) as u32
    }
    /// Generate a Java-compatible bounded random number.
    #[allow(
        clippy::modulo_arithmetic,
        reason = "Java signed remainder is part of the native procedural algorithm"
    )]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::integer_division_remainder_used,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    fn next(&mut self, bound: i32) -> Result<i32> {
        ensure!(bound > 0_i32, "Invalid random bound");
        if (bound as u32).is_power_of_two() {
            return Ok(((bound as u64 * u64::from(self.bits())) >> 32) as i32);
        }
        let max = (i64::from(i32::MIN) - (0x0001_0000_0000_u64 % bound as u64) as i64) as i32;
        loop {
            let v = self.bits() as i32;
            if v < max {
                let sign = (v >> 31) & (bound - 1);
                return Ok(sign + v.wrapping_add((v as u32 >> 31) as i32) % bound);
            }
        }
    }
}
/// Build the native seeded Perlin permutation table.
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
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn permutations(seed: i32) -> Result<[u8; 512]> {
    let mut p = [0; 512];
    for (i, v) in p.iter_mut().enumerate().take(255) {
        *v = i as u8;
    }
    let mut rng = Random::new(seed);
    for i in 0..255 {
        let j = 255 - i;
        let k = rng.next(j as i32)? as usize;
        let v = p[k];
        p.swap(k, j);
        p[511 - i] = v;
    }
    Ok(p)
}
/// Construct a native 4096-step remapping curve.
#[allow(
    clippy::indexing_slicing,
    reason = "indices follow validated native tables and allocated image dimensions"
)]
#[allow(
    clippy::missing_asserts_for_indexing,
    reason = "native table indices are already constrained by the decoding and loop bounds"
)]
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
fn curve(op: &Op) -> Result<Vec<i32>> {
    let default = vec![0_i32, 0_i32, 0_i32, 4_096_i32, 4_096_i32];
    let data = op.fields.get(&0).unwrap_or(&default);
    let mode = data[0];
    let m = data[1..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| [v[0], v[1]])
        .collect::<Vec<_>>();
    ensure!(
        m.len() >= 2 && m.windows(2).all(|w| w[0][0] <= w[1][0]),
        "Invalid curve markers: {m:?}"
    );
    let mut table = vec![];
    for i in 0_i32..257_i32 {
        let x = i * 16_i32;
        let j = (1..m.len() - 1)
            .find(|&j| m[j][0] > x)
            .unwrap_or(m.len() - 1);
        let a = m[j - 1];
        let b = m[j];
        let t = (x - a[0]) * 4_096_i32 / (b[0] - a[0]);
        let value = match mode {
            0_i32 => (a[1] * (4_096_i32 - t) + b[1] * t) / 4_096_i32,
            1_i32 => {
                let aidx = f64::from((t & 0x1ffb_i32) / 32_i32);
                let w = (4_096_i32
                    - (aidx / 255. * core::f64::consts::TAU)
                        .cos()
                        .mul_add(4096., 0.) as i32)
                    / 2_i32;
                (a[1] * (4_096_i32 - w) + b[1] * w) / 4_096_i32
            }
            2_i32 => {
                let prev = if j >= 2 {
                    m[j - 2][1]
                } else {
                    2_i32 * m[0][1] - m[1][1]
                };
                let next = if j + 1 < m.len() {
                    m[j + 1][1]
                } else {
                    2_i32 * m[m.len() - 2][1] - m[m.len() - 1][1]
                };
                let q = t * t / 4_096_i32;
                let aa = a[1] - prev + next - b[1];
                let bb = prev - a[1] - aa;
                let cc = b[1] - prev;
                ((q * ((t * aa) >> 12_i32)) >> 12_i32)
                    + q * bb / 4_096_i32
                    + t * cc / 4_096_i32
                    + a[1]
            }
            n => bail!("Unknown curve interpolation {n}"),
        };
        table.push(value.clamp(-32_767_i32, 32_767_i32));
    }
    Ok(table)
}
/// Construct the native fixed-point color-gradient lookup table.
#[allow(
    clippy::missing_asserts_for_indexing,
    reason = "native table indices are already constrained by the decoding and loop bounds"
)]
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
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn gradient(data: &[i32]) -> Result<Vec<[i32; 3]>> {
    let (markers, remainder) = data.as_chunks::<4>();
    ensure!(
        remainder.is_empty() && !markers.is_empty(),
        "Invalid gradient marker count"
    );
    let m = markers.iter().collect::<Vec<_>>();
    ensure!(
        m.windows(2).all(|w| w[0][0] <= w[1][0]),
        "Invalid gradient markers"
    );
    let mut table = vec![];
    for i in 0_i32..257_i32 {
        let x = i * 16_i32;
        let mut out = [0_i32; 3];
        if x <= m[0][0] {
            out.copy_from_slice(&m[0][1..]);
        } else if x >= m[m.len() - 1][0] {
            out.copy_from_slice(&m[m.len() - 1][1..]);
        } else {
            let j = (1..m.len())
                .find(|&j| m[j][0] > x)
                .context("Missing gradient interval")?;
            let t = (x - m[j - 1][0]) * 4_096_i32 / (m[j][0] - m[j - 1][0]);
            for c in 0..3 {
                out[c] = (m[j - 1][c + 1] * (4_096_i32 - t) + m[j][c + 1] * t) >> 12_i32;
            }
        }
        table.push(out);
    }
    Ok(table)
}
/// Evaluate the fixed-point Perlin interpolation polynomial.
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
const fn fade(t: i32) -> i32 {
    let cubic = (((t * t) >> 12_i32) * t) >> 12_i32;
    let factor = 40_960_i32 + (((6_i32 * t - 61_440_i32) * t) >> 12_i32);
    (factor * cubic) >> 12
}
/// Generate native multi-octave Perlin noise.
#[allow(
    clippy::indexing_slicing,
    reason = "indices follow validated native tables and allocated image dimensions"
)]
#[allow(
    clippy::many_single_char_names,
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::float_arithmetic,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn perlin(op: &Op, out: &mut Image, n: usize) -> Result<()> {
    let octaves = op.v(1, 4) as usize;
    ensure!((1..=12).contains(&octaves), "Invalid octave count");
    let persistence = op.v(2, 1638);
    let mut amps = vec![];
    for i in 0..octaves {
        let a = if persistence > 0_i32 {
            i32::from(
                (f64::from(persistence as f32 / 4096.).powi(i as i32) * 4_096.0_f64) as i32 as i16,
            )
        } else {
            *op.fields
                .get(&2)
                .and_then(|v| v.get(i + 1))
                .context("Missing octave amplitude")?
        };
        amps.push(a);
    }
    while amps.len() > 1
        && amps
            .last()
            .is_some_and(|amplitude| amplitude.abs() <= 8_i32)
    {
        let _previous = amps.pop();
    }
    let sx = op.v(5, op.v(3, 4));
    let sy = op.v(6, op.v(3, 4));
    ensure!(sx > 0_i32 && sy > 0_i32, "Invalid noise scale");
    let p = permutations(op.v(4, 0))?;
    for y in 0..n {
        for x in 0..n {
            let mut value = 0_i32;
            for (oct, &amp) in amps.iter().enumerate() {
                if amp.abs() <= 8_i32 {
                    continue;
                }
                let freq = 1_i32 << oct;
                let xx = x as i32 * 4_096_i32 / n as i32 * sx * freq;
                let yy = y as i32 * 4_096_i32 / n as i32 * sy * freq;
                let (ix, iy) = (xx >> 12_i32, yy >> 12_i32);
                let (nx, ny) = (
                    if ix + 1_i32 >= sx * freq {
                        0_i32
                    } else {
                        ix + 1_i32
                    },
                    if iy + 1_i32 >= sy * freq {
                        0_i32
                    } else {
                        iy + 1_i32
                    },
                );
                let (fx, fy) = (xx & 4_095_i32, yy & 4_095_i32);
                let grad = |a: i32, b: i32, dx: i32, dy: i32| {
                    let h =
                        p[(i32::from(p[(b & 0xff_i32) as usize]) + (a & 0xff_i32)) as usize] & 3;
                    (if h & 1 == 0 { dx } else { -dx }) + (if h & 2 == 0 { dy } else { -dy })
                };
                let a = grad(ix, iy, fx, fy);
                let b = grad(nx, iy, fx - 4_096_i32, fy);
                let c = grad(ix, ny, fx, fy - 4_096_i32);
                let d = grad(nx, ny, fx - 4_096_i32, fy - 4_096_i32);
                let u = fade(fx);
                let v = fade(fy);
                let ab = a + ((u * (b - a)) >> 12_i32);
                let cd = c + ((u * (d - c)) >> 12_i32);
                value += ((ab + ((v * (cd - ab)) >> 12_i32)) * amp) >> 12_i32;
            }
            if op.v(0, 1) == 1_i32 {
                value = (value >> 1_i32) + 2_048_i32;
            }
            out[y * n + x] = [value; 3];
        }
    }
    Ok(())
}
/// Generate the native cellular noise texture operation.
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
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::float_arithmetic,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn voronoi(op: &Op, out: &mut Image, n: usize) -> Result<()> {
    let seed = op.v(1, 0);
    let p = permutations(seed)?;
    let mut rng = Random::new(seed);
    let mut offsets = [0_i32; 512];
    if op.v(2, 2048) > 0_i32 {
        for v in &mut offsets {
            *v = rng.next(op.v(2, 2048))?;
        }
    }
    let (sx, sy) = (op.v(5, op.v(0, 5)), op.v(6, op.v(0, 5)));
    ensure!(sx > 0_i32 && sy > 0_i32, "Invalid Voronoi scale");
    for y in 0..n {
        for x in 0..n {
            let xx = x as i32 * 4_096_i32 / n as i32 * sx + 2_048_i32;
            let yy = y as i32 * 4_096_i32 / n as i32 * sy + 2_048_i32;
            let mut near = [i32::MAX; 4];
            for j in (yy >> 12_i32) - 1_i32..=(yy >> 12_i32) + 1_i32 {
                let py = i32::from(p[((if j >= sy { j - sy } else { j }) & 0xff_i32) as usize]);
                for i in (xx >> 12_i32) - 1_i32..=(xx >> 12_i32) + 1_i32 {
                    let idx = p[(((if i >= sx { i - sx } else { i }) + py) & 0xff_i32) as usize]
                        as usize
                        * 2;
                    let dx = xx - (offsets[idx] + i * 4_096_i32);
                    let dy = yy - (offsets[idx + 1] + j * 4_096_i32);
                    let dist = match op.v(4, 1) {
                        1_i32 => (dx * dx + dy * dy) >> 12_i32,
                        2_i32 => dx.abs() + dy.abs(),
                        3_i32 => dx.abs().max(dy.abs()),
                        4_i32 => {
                            let a = ((dx.abs() as f32 / 4096.).sqrt() * 4096.) as i32;
                            let b = ((dy.abs() as f32 / 4096.).sqrt() * 4096.) as i32;
                            ((a + b) * (a + b)) >> 12_i32
                        }
                        5_i32 => {
                            (((dx * dx + dy * dy) as f32 / 16_777_216.).sqrt().sqrt() * 4096.)
                                as i32
                        }
                        _ => (((dx * dx + dy * dy) as f32 / 16_777_216.).sqrt() * 4096.) as i32,
                    };
                    if let Some(k) = near.iter().position(|&v| dist < v) {
                        for j in (k + 1..4).rev() {
                            near[j] = near[j - 1];
                        }
                        near[k] = dist;
                    }
                }
            }
            let v = match op.v(3, 2) {
                0_i32 => near[0],
                1_i32 => near[1],
                2_i32 => near[1] - near[0],
                3_i32 => near[2],
                4_i32 => near[3],
                n => bail!("Unknown Voronoi mode {n}"),
            };
            out[y * n + x] = [v; 3];
        }
    }
    Ok(())
}
/// Shade native fixed-point normals using the emboss operation.
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
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::float_arithmetic,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn emboss(op: &Op, input: &Image, out: &mut Image, n: usize) {
    let az = op.v(1, 3216) as f32 / 4096.;
    let el = op.v(2, 3216) as f32 / 4096.;
    let mut light = [
        (4096. * el.cos() * az.sin()) as i32,
        (4096. * el.cos() * az.cos()) as i32,
        (4096. * el.sin()) as i32,
    ];
    let s = f64::from(
        (((light[0] * light[0]) >> 12_i32)
            + ((light[1] * light[1]) >> 12_i32)
            + ((light[2] * light[2]) >> 12_i32))
            >> 12_i32,
    );
    let scale = (s.sqrt() * 4_096.0_f64) as i32;
    if scale != 0_i32 {
        for v in &mut light {
            *v = (*v << 12_i32) / scale;
        }
    }
    let mult = (op.v(0, 4096) * (n as i32 * 32_i32)) >> 12_i32;
    for y in 0..n {
        for x in 0..n {
            let dx = (mult
                * (input[y * n + wrap(x as i32 - 1, n)][0]
                    - input[y * n + wrap(x as i32 + 1, n)][0]))
                >> 12_i32;
            let dy = (mult
                * (input[wrap(y as i32 + 1, n) * n + x][0]
                    - input[wrap(y as i32 - 1, n) * n + x][0]))
                >> 12_i32;
            let a = (dx >> 4).abs().min(255_i32);
            let b = (dy >> 4).abs().min(255_i32);
            let inv = (255. / ((a * a + b * b + 65535) as f32 / 65535.).sqrt()) as i32;
            let value = ((light[0] * ((inv * dx) >> 8_i32)) >> 12_i32)
                + ((light[1] * ((inv * dy) >> 8_i32)) >> 12_i32)
                + ((light[2] * ((inv * 4_096_i32) >> 8_i32)) >> 12_i32);
            out[y * n + x] = [value; 3];
        }
    }
}
/// Generate the native staggered brick texture operation.
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
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn bricks(op: &Op, out: &mut Image, n: usize) -> Result<()> {
    let cols = op.v(0, 4) as usize;
    let rows = op.v(1, 8) as usize;
    ensure!(cols > 0 && rows > 0, "Invalid brick grid");
    let mut rng = Random::new(rows as i32);
    let w = 4_096_i32 / cols as i32;
    let h = 4_096_i32 / rows as i32;
    let mut ys = vec![0_i32; rows + 1];
    let mut xs = vec![vec![0_i32; cols + 1]; rows];
    let mut values = vec![vec![0_i32; cols]; rows];
    for row in 0..rows {
        if row > 0 {
            ys[row] = ys[row - 1]
                + h
                + (((((rng.next(4096)? - 2_048_i32) * op.v(3, 204)) >> 12_i32) * (h / 2_i32))
                    >> 12_i32);
        }
        for col in 0..cols {
            if col > 0 {
                xs[row][col] = xs[row][col - 1]
                    + w
                    + (((((rng.next(4096)? - 2_048_i32) * op.v(2, 409)) >> 12_i32) * (w / 2_i32))
                        >> 12_i32);
            }
            values[row][col] = if op.v(7, 1024) > 0_i32 {
                4_096_i32 - rng.next(op.v(7, 1024))?
            } else {
                4_096_i32
            };
        }
        xs[row][cols] = 4_096_i32;
    }
    ys[rows] = 4_096_i32;
    let margin = op.v(6, 81) / 2_i32;
    for y in 0..n {
        let yy = (y as i32 * 4096 / n as i32 + op.v(5, 0)).rem_euclid(4096);
        let row = (1..=rows).find(|&r| yy < ys[r]).unwrap_or(rows) - 1;
        for x in 0..n {
            let offset = if (row + 1) % 2 != 0 {
                -op.v(4, 1024)
            } else {
                op.v(4, 1024)
            };
            let xx = (x as i32 * 4096 / n as i32 + ((w * offset) >> 12)).rem_euclid(4096);
            let col = (1..=cols).find(|&c| xx < xs[row][c]).unwrap_or(cols) - 1;
            let v = if yy > ys[row] + margin
                && yy < ys[row + 1] - margin
                && xx > xs[row][col] + margin
                && xx < xs[row][col + 1] - margin
            {
                values[row][col]
            } else {
                0_i32
            };
            out[y * n + x] = [v; 3];
        }
    }
    Ok(())
}
/// Generate native seeded line-noise strokes.
#[allow(
    clippy::indexing_slicing,
    reason = "indices follow validated native tables and allocated image dimensions"
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
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::float_arithmetic,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn lines(op: &Op, out: &mut Image, n: usize) -> Result<()> {
    let mut rng = Random::new(op.v(0, 0));
    let spread = op.v(4, 4096);
    for _ in 0_i32..op.v(1, 2000) {
        let a = if spread > 0_i32 {
            op.v(3, 0) - spread / 2_i32 + rng.next(spread)?
        } else {
            op.v(3, 0)
        };
        let angle = f64::from((a >> 4_i32) & 0xff_i32) / 255.0_f64 * core::f64::consts::TAU;
        let (mut x0, mut y0) = (rng.next(n as i32)?, rng.next(n as i32)?);
        let (mut x1, mut y1) = (
            x0 + (((angle.cos() * 4_096.0_f64) as i32 * op.v(2, 16)) >> 12_i32),
            y0 + (((angle.sin() * 4_096.0_f64) as i32 * op.v(2, 16)) >> 12_i32),
        );
        let steep = (x1 - x0).abs() < (y1 - y0).abs();
        if x0 == x1 && y0 == y1 {
            continue;
        }
        if steep {
            core::mem::swap(&mut x0, &mut y0);
            core::mem::swap(&mut x1, &mut y1);
        }
        if x0 > x1 {
            core::mem::swap(&mut x0, &mut x1);
            core::mem::swap(&mut y0, &mut y1);
        }
        let dx = x1 - x0;
        let dy = (y1 - y0).abs();
        let mut error = -dx / 2_i32;
        let direction = if y1 <= y0 { -1_i32 } else { 1_i32 };
        let slope = 2_048_i32 / dx;
        let base = 2_048_i32 - (rng.next(4096)? >> 2_i32);
        let mut yy = y0;
        for x in x0..x1 {
            error += dy;
            let value = slope * (x - x0) + base;
            let line = wrap(yy, n);
            if error > 0_i32 {
                yy += direction;
                error -= dx;
            }
            let pixel = wrap(x, n);
            let index = if steep {
                line * n + pixel
            } else {
                pixel * n + line
            };
            out[index] = [value; 3];
        }
    }
    Ok(())
}
#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "test fixtures use exact values, fixed indexes and panic on unexpected failures"
)]
mod tests {
    use super::*;
    #[test]
    fn perlin_respects_native_resolution_without_resizing() {
        let op = Op {
            kind: 34,
            fields: BTreeMap::new(),
            inputs: vec![],
            mono: true,
        };
        let mut small = vec![[0_i32; 3]; 64 * 64];
        let mut large = vec![[0_i32; 3]; 128 * 128];
        perlin(&op, &mut small, 64).unwrap();
        perlin(&op, &mut large, 128).unwrap();
        for y in 0..64 {
            for x in 0..64 {
                assert_eq!(small[y * 64 + x], large[y * 2 * 128 + x * 2]);
            }
        }
        assert_eq!(wrap(-1, 64), 63);
        assert_eq!(wrap(-1, 128), 127);
    }
    #[test]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    fn java_random_sequence() {
        let mut r = Random::new(0);
        assert_eq!(r.bits(), 3_139_482_720);
        assert_eq!(r.bits(), 3_571_011_896);
    }
    #[test]
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    fn linear_curve() {
        let o = Op {
            kind: 8,
            fields: BTreeMap::new(),
            inputs: vec![],
            mono: true,
        };
        let c = curve(&o).unwrap();
        assert_eq!(c[0], 0_i32);
        assert_eq!(c[128], 2_048_i32);
        assert_eq!(c[256], 4_096_i32);
    }
}
