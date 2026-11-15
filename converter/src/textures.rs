// SPDX-License-Identifier: AGPL-3.0-only
//! Offline revision-530 procedural textures. Fixed-point operations adapted from
//! rs-map-viewer (BSD-2-Clause; see third_party/rs-map-viewer-LICENSE).
use crate::{binary::Reader, cache::Cache};
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
type Image = Vec<[i32; 3]>;
#[derive(Clone)]
struct Op {
    kind: u8,
    fields: BTreeMap<u8, Vec<i32>>,
    inputs: Vec<usize>,
    mono: bool,
}
impl Op {
    fn v(&self, id: u8, default: i32) -> i32 {
        self.fields
            .get(&id)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(default)
    }
}
#[derive(Clone)]
pub struct Texture {
    pub pixels: Vec<[u8; 4]>,
    pub size: usize,
    pub effect: u8,
    pub brightness: u8,
    pub boost: u8,
    dependency: Image,
}
pub struct Textures {
    pub images: BTreeMap<i32, Texture>,
    effects: Vec<u8>,
    sizes: Vec<usize>,
    brightness: Vec<u8>,
    boost: Vec<u8>,
    active: HashSet<i32>,
}
#[cfg(test)]
impl Textures {
    pub fn fixture(images: BTreeMap<i32, (usize, Vec<[u8; 4]>)>) -> Self {
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
    pub fn new(cache: &mut Cache) -> Result<Self> {
        let bytes = cache.file(26, 0, 0)?;
        let mut r = Reader::new(&bytes);
        let count = r.u16()? as usize;
        let exists = (0..count)
            .map(|_| r.u8().map(|b| b == 1))
            .collect::<Result<Vec<_>>>()?;
        let mut effects = vec![0; count];
        let mut sizes = vec![128; count];
        let mut brightness = vec![0; count];
        let mut boost = vec![0; count];
        for field in 0..9 {
            for i in 0..count {
                if !exists[i] {
                    continue;
                }
                if field == 8 {
                    r.u16()?;
                } else {
                    let b = r.u8()?;
                    match field {
                        2 => sizes[i] = if b == 1 { 64 } else { 128 },
                        6 => effects[i] = b,
                        5 => brightness[i] = b,
                        4 => boost[i] = b,
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
    pub fn load(&mut self, cache: &mut Cache, id: i32) -> Result<()> {
        if self.images.contains_key(&id) {
            return Ok(());
        }
        ensure!(self.active.insert(id), "Cyclic texture dependency {id}");
        let result = self.generate(cache, id);
        self.active.remove(&id);
        let image = result.with_context(|| format!("Procedural texture {id}"))?;
        self.images.insert(id, image);
        Ok(())
    }
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
            r.u8()?;
            let kind = r.u8()?;
            r.u8()?;
            let ct = r.u8()?;
            let mut fields = BTreeMap::new();
            for _ in 0..ct {
                let f = r.u8()?;
                let values = match (kind, f) {
                    (8, 0) => {
                        let mode = r.u8()? as i32;
                        let count = r.u8()? as usize;
                        let mut v = vec![mode];
                        for _ in 0..count {
                            v.push(r.u16()? as i32);
                            v.push(r.u16()? as i32);
                        }
                        v
                    }
                    (10, 0) => {
                        let preset = r.u8()?;
                        ensure!(preset == 0, "Gradient preset {preset} not yet supported");
                        let count = r.u8()?;
                        let mut v = vec![];
                        for _ in 0..count {
                            v.push(r.u16()? as i32);
                            for _ in 0..3 {
                                v.push((r.u8()? as i32) << 4);
                            }
                        }
                        v
                    }
                    (34, 2) => {
                        let persistence = r.u16()? as i16 as i32;
                        let mut v = vec![persistence];
                        if persistence < 0 {
                            let oct = fields
                                .get(&1)
                                .and_then(|v: &Vec<i32>| v.first())
                                .copied()
                                .unwrap_or(4);
                            for _ in 0..oct {
                                v.push(r.u16()? as i16 as i32);
                            }
                        }
                        v
                    }
                    (1, 0) => vec![r.u24()? as i32],
                    (5, 0..=2)
                    | (0, 0)
                    | (4, 0..=1)
                    | (7, 0..=1)
                    | (6, 2)
                    | (9, 0..=2)
                    | (19, 1)
                    | (20, 0..=1)
                    | (21, 0)
                    | (22, 0)
                    | (23, 0)
                    | (30, 2)
                    | (34, 0..=1)
                    | (34, 3..=6)
                    | (15, 0..=1)
                    | (15, 3..=6)
                    | (38, 0)
                    | (38, 2) => vec![r.u8()? as i32],
                    (4, 2..=7)
                    | (6, 0..=1)
                    | (15, 2)
                    | (19, 0)
                    | (30, 0..=1)
                    | (32, 0..=2)
                    | (36, 0)
                    | (38, 1)
                    | (38, 3..=4)
                    | (39, 0)
                    | (18, 0) => vec![r.u16()? as i32],
                    _ => bail!("Unsupported texture operation {kind} field {f}"),
                };
                fields.insert(f, values);
            }
            let input_count = match kind {
                0..=4 | 13 | 15 | 18 | 34 | 36 | 38 | 39 => 0,
                7 => 2,
                19 | 21 => 3,
                5 | 6 | 8..=10 | 20 | 22..=24 | 30 | 32 => 1,
                _ => bail!("Unsupported texture operation {kind}"),
            };
            let inputs = (0..input_count)
                .map(|_| r.u8().map(|v| v as usize))
                .collect::<Result<Vec<_>>>()?;
            let mono = match kind {
                0 | 2 | 3 | 4 | 8 | 13 | 15 | 32 | 34 | 38 => true,
                6 => fields.get(&2).is_some_and(|v| v[0] == 1),
                7 => fields.get(&1).is_some_and(|v| v[0] == 1),
                19 => fields.get(&1).is_some_and(|v| v[0] == 1),
                21 => fields.get(&0).is_some_and(|v| v[0] == 1),
                30 => fields.get(&2).is_some_and(|v| v[0] == 1),
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
        r.take(7)?;
        r.finish()?;
        let mut dependency = vec![[0; 3]; n * n];
        for y in 0..n {
            for x in 0..n {
                let dst = if flip { x * n + y } else { y * n + x };
                dependency[dst] = c[y * n + x].map(|v| (v >> 4).clamp(0, 255) * 16);
            }
        }
        let mut pixels = vec![[0; 4]; n * n];
        for y in 0..n {
            for x in 0..n {
                let src = y * n + (n - 1 - x);
                let dst = if flip { x * n + y } else { y * n + x };
                for channel in 0..3 {
                    pixels[dst][channel] = (((c[src][channel] >> 4).clamp(0, 255) as f64 / 255.)
                        .powf(0.7)
                        * 255.) as u8;
                }
                pixels[dst][3] = if pixels[dst][..3] == [0, 0, 0] {
                    0
                } else {
                    (a[src][0] >> 4).clamp(0, 255) as u8
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
            return Ok(img.clone());
        }
        ensure!(visiting.insert(index), "Cyclic texture graph");
        let op = ops.get(index).context("Invalid texture input")?;
        let mut input = vec![];
        for &i in &op.inputs {
            input.push(self.eval(cache, ops, i, results, visiting, n)?);
        }
        let mut out = vec![[0; 3]; n * n];
        let scalar = |v: i32| [v; 3];
        match op.kind {
            0 => out.fill(scalar(op.v(0, 255) * 4096 / 255)),
            1 => {
                let rgb = op.v(0, 0);
                out.fill([
                    (rgb >> 16 & 255) * 16,
                    (rgb >> 8 & 255) * 16,
                    (rgb & 255) * 16,
                ]);
            }
            2 | 3 => {
                for y in 0..n {
                    for x in 0..n {
                        out[y * n + x] =
                            scalar((if op.kind == 2 { x } else { y }) as i32 * 4096 / n as i32);
                    }
                }
            }
            4 => bricks(op, &mut out, n)?,
            5 => {
                let hx = op.v(0, 1);
                let hy = op.v(1, 1);
                ensure!(
                    (0..=32).contains(&hx) && (0..=32).contains(&hy),
                    "Blur too large"
                );
                let mut pass = vec![[0; 3]; n * n];
                let ix = 65536 / (2 * hx + 1);
                let iy = 65536 / (2 * hy + 1);
                for y in 0..n {
                    for x in 0..n {
                        for c in 0..3 {
                            let sum = (-hx..=hx)
                                .map(|dx| input[0][y * n + wrap(x as i32 + dx, n)][c])
                                .sum::<i32>();
                            pass[y * n + x][c] = sum * ix / 65536;
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
                                / 65536;
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
                            1 => a + b,
                            2 => a - b,
                            3 => a * b / 4096,
                            4 => {
                                if b == 0 {
                                    4096
                                } else {
                                    a * 4096 / b
                                }
                            }
                            5 => 4096 - (4096 - a) * (4096 - b) / 4096,
                            6 => {
                                if b < 2048 {
                                    b * a / 2048
                                } else {
                                    4096 - (4096 - a) * (4096 - b) / 2048
                                }
                            }
                            7 => {
                                if a == 4096 {
                                    4096
                                } else {
                                    b * 4096 / (4096 - a)
                                }
                            }
                            8 => {
                                if a == 0 {
                                    0
                                } else {
                                    4096 - (4096 - b) * 4096 / a
                                }
                            }
                            9 => a.min(b),
                            10 => a.max(b),
                            11 => (a - b).abs(),
                            12 => a + b - a * b / 2048,
                            n => bail!("Unknown arithmetic mode {n}"),
                        };
                    }
                }
            }
            8 => {
                let table = curve(op)?;
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = scalar(table[(i[0] / 16).clamp(0, 256) as usize]);
                }
            }
            9 => {
                for y in 0..n {
                    for x in 0..n {
                        let xx = if op.v(0, 1) == 1 { n - 1 - x } else { x };
                        let yy = if op.v(1, 1) == 1 { n - 1 - y } else { y };
                        out[y * n + x] = input[0][yy * n + xx];
                    }
                }
            }
            10 => {
                let markers = op.fields.get(&0).context("Missing colour gradient")?;
                ensure!(markers.len() >= 4, "Gradient needs two markers");
                let table = gradient(markers)?;
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = table[(i[0] >> 4).clamp(0, 256) as usize];
                }
            }
            13 => {
                for y in 0..n {
                    for x in 0..n {
                        let hash_input =
                            (x as i32 * 4096 / n as i32) + (y as i32 * 4096 / n as i32) * 57;
                        let h = hash_input ^ (hash_input << 1);
                        let v = h
                            .wrapping_mul(h)
                            .wrapping_mul(15731)
                            .wrapping_add(789221)
                            .wrapping_mul(h)
                            .wrapping_add(1376312589)
                            & 0x7fffffff;
                        out[y * n + x] = scalar((4096 - v / 262144) % 4096);
                    }
                }
            }
            15 => voronoi(op, &mut out, n)?,
            19 => {
                for y in 0..n {
                    for x in 0..n {
                        let i = y * n + x;
                        let a = (input[1][i][0] >> 4) & 255;
                        let angle = a as f64 / 255. * std::f64::consts::TAU;
                        let amp = (input[2][i][0] * op.v(0, 32768) * 16) >> 12;
                        let xx = x as i32 + ((((angle.cos() * 4096.) as i32 * amp) >> 12) >> 12);
                        let yy = y as i32 + ((((angle.sin() * 4096.) as i32 * amp) >> 12) >> 12);
                        out[i] = input[0][wrap(yy, n) * n + wrap(xx, n)];
                    }
                }
            }
            20 => {
                let (h, v) = (op.v(0, 4), op.v(1, 4));
                ensure!(
                    h > 0 && v > 0 && h <= n as i32 && v <= n as i32,
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
                        *v = (input[0][i][c] * w + input[1][i][c] * (4096 - w)) >> 12;
                    }
                }
            }
            22 => {
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = i.map(|v| 4096 - v);
                }
            }
            24 => {
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = scalar((i[0] + i[1] + i[2]) / 3);
                }
            }
            30 => {
                let a = op.v(0, 1024);
                let b = op.v(1, 3072);
                for (o, i) in out.iter_mut().zip(input[0].iter()) {
                    *o = i.map(|v| a + ((v * (b - a)) >> 12));
                }
            }
            32 => emboss(op, &input[0], &mut out, n),
            34 => perlin(op, &mut out, n)?,
            36 => {
                let id = op.v(0, -1);
                ensure!(id >= 0, "Missing texture dependency");
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
        visiting.remove(&index);
        let out = Arc::new(out);
        results.insert(index, out.clone());
        Ok(out)
    }
}
fn wrap(x: i32, n: usize) -> usize {
    (x as usize) & (n - 1)
}
struct Random(u64);
impl Random {
    fn new(seed: i32) -> Self {
        Self((seed as u64 ^ 0x5deece66d) & ((1 << 48) - 1))
    }
    fn bits(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x5deece66d).wrapping_add(11) & ((1 << 48) - 1);
        (self.0 >> 16) as u32
    }
    fn next(&mut self, bound: i32) -> Result<i32> {
        ensure!(bound > 0, "Invalid random bound");
        if (bound as u32).is_power_of_two() {
            return Ok(((bound as u64 * self.bits() as u64) >> 32) as i32);
        }
        let max = (i32::MIN as i64 - (0x100000000u64 % bound as u64) as i64) as i32;
        loop {
            let v = self.bits() as i32;
            if v < max {
                let sign = (v >> 31) & (bound - 1);
                return Ok(sign + v.wrapping_add((v as u32 >> 31) as i32) % bound);
            }
        }
    }
}
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
fn curve(op: &Op) -> Result<Vec<i32>> {
    let default = vec![0, 0, 0, 4096, 4096];
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
    for i in 0..257 {
        let x = i * 16;
        let j = (1..m.len() - 1)
            .find(|&j| m[j][0] > x)
            .unwrap_or(m.len() - 1);
        let a = m[j - 1];
        let b = m[j];
        let t = (x - a[0]) * 4096 / (b[0] - a[0]);
        let value = match mode {
            0 => (a[1] * (4096 - t) + b[1] * t) / 4096,
            1 => {
                let aidx = ((t & 8187) / 32) as f64;
                let w = (4096
                    - (aidx / 255. * std::f64::consts::TAU)
                        .cos()
                        .mul_add(4096., 0.) as i32)
                    / 2;
                (a[1] * (4096 - w) + b[1] * w) / 4096
            }
            2 => {
                let prev = if j >= 2 {
                    m[j - 2][1]
                } else {
                    2 * m[0][1] - m[1][1]
                };
                let next = if j + 1 < m.len() {
                    m[j + 1][1]
                } else {
                    2 * m[m.len() - 2][1] - m[m.len() - 1][1]
                };
                let q = t * t / 4096;
                let aa = a[1] - prev + next - b[1];
                let bb = prev - a[1] - aa;
                let cc = b[1] - prev;
                ((q * ((t * aa) >> 12)) >> 12) + q * bb / 4096 + t * cc / 4096 + a[1]
            }
            n => bail!("Unknown curve interpolation {n}"),
        };
        table.push(value.clamp(-32767, 32767));
    }
    Ok(table)
}
fn gradient(data: &[i32]) -> Result<Vec<[i32; 3]>> {
    let m = data.as_chunks::<4>().0.iter().collect::<Vec<_>>();
    ensure!(
        m.windows(2).all(|w| w[0][0] <= w[1][0]),
        "Invalid gradient markers"
    );
    let mut table = vec![];
    for i in 0..257 {
        let x = i * 16;
        let mut out = [0; 3];
        if x <= m[0][0] {
            out.copy_from_slice(&m[0][1..]);
        } else if x >= m[m.len() - 1][0] {
            out.copy_from_slice(&m[m.len() - 1][1..]);
        } else {
            let j = (1..m.len()).find(|&j| m[j][0] > x).unwrap();
            let t = (x - m[j - 1][0]) * 4096 / (m[j][0] - m[j - 1][0]);
            for c in 0..3 {
                out[c] = (m[j - 1][c + 1] * (4096 - t) + m[j][c + 1] * t) >> 12;
            }
        }
        table.push(out);
    }
    Ok(table)
}
fn fade(t: i32) -> i32 {
    let cubic = (((t * t) >> 12) * t) >> 12;
    let factor = 40960 + (((6 * t - 61440) * t) >> 12);
    (factor * cubic) >> 12
}
fn perlin(op: &Op, out: &mut Image, n: usize) -> Result<()> {
    let octaves = op.v(1, 4) as usize;
    ensure!((1..=12).contains(&octaves), "Invalid octave count");
    let persistence = op.v(2, 1638);
    let mut amps = vec![];
    for i in 0..octaves {
        let a = if persistence > 0 {
            (((persistence as f32 / 4096.) as f64).powi(i as i32) * 4096.) as i32 as i16 as i32
        } else {
            *op.fields
                .get(&2)
                .and_then(|v| v.get(i + 1))
                .context("Missing octave amplitude")?
        };
        amps.push(a);
    }
    while amps.len() > 1 && amps.last().unwrap().abs() <= 8 {
        amps.pop();
    }
    let sx = op.v(5, op.v(3, 4));
    let sy = op.v(6, op.v(3, 4));
    ensure!(sx > 0 && sy > 0, "Invalid noise scale");
    let p = permutations(op.v(4, 0))?;
    for y in 0..n {
        for x in 0..n {
            let mut value = 0;
            for (oct, &amp) in amps.iter().enumerate() {
                if amp.abs() <= 8 {
                    continue;
                }
                let freq = 1 << oct;
                let xx = x as i32 * 4096 / n as i32 * sx * freq;
                let yy = y as i32 * 4096 / n as i32 * sy * freq;
                let (ix, iy) = (xx >> 12, yy >> 12);
                let (nx, ny) = (
                    if ix + 1 >= sx * freq { 0 } else { ix + 1 },
                    if iy + 1 >= sy * freq { 0 } else { iy + 1 },
                );
                let (fx, fy) = (xx & 4095, yy & 4095);
                let grad = |a: i32, b: i32, dx: i32, dy: i32| {
                    let h = p[(p[(b & 255) as usize] as i32 + (a & 255)) as usize] & 3;
                    (if h & 1 == 0 { dx } else { -dx }) + (if h & 2 == 0 { dy } else { -dy })
                };
                let a = grad(ix, iy, fx, fy);
                let b = grad(nx, iy, fx - 4096, fy);
                let c = grad(ix, ny, fx, fy - 4096);
                let d = grad(nx, ny, fx - 4096, fy - 4096);
                let u = fade(fx);
                let v = fade(fy);
                let ab = a + ((u * (b - a)) >> 12);
                let cd = c + ((u * (d - c)) >> 12);
                value += ((ab + ((v * (cd - ab)) >> 12)) * amp) >> 12;
            }
            if op.v(0, 1) == 1 {
                value = (value >> 1) + 2048;
            }
            out[y * n + x] = [value; 3];
        }
    }
    Ok(())
}
fn voronoi(op: &Op, out: &mut Image, n: usize) -> Result<()> {
    let seed = op.v(1, 0);
    let p = permutations(seed)?;
    let mut rng = Random::new(seed);
    let mut offsets = [0; 512];
    if op.v(2, 2048) > 0 {
        for v in &mut offsets {
            *v = rng.next(op.v(2, 2048))?;
        }
    }
    let (sx, sy) = (op.v(5, op.v(0, 5)), op.v(6, op.v(0, 5)));
    ensure!(sx > 0 && sy > 0, "Invalid Voronoi scale");
    for y in 0..n {
        for x in 0..n {
            let xx = x as i32 * 4096 / n as i32 * sx + 2048;
            let yy = y as i32 * 4096 / n as i32 * sy + 2048;
            let mut near = [i32::MAX; 4];
            for j in (yy >> 12) - 1..=(yy >> 12) + 1 {
                let py = p[((if j >= sy { j - sy } else { j }) & 255) as usize] as i32;
                for i in (xx >> 12) - 1..=(xx >> 12) + 1 {
                    let idx =
                        p[(((if i >= sx { i - sx } else { i }) + py) & 255) as usize] as usize * 2;
                    let dx = xx - (offsets[idx] + i * 4096);
                    let dy = yy - (offsets[idx + 1] + j * 4096);
                    let dist = match op.v(4, 1) {
                        1 => (dx * dx + dy * dy) >> 12,
                        2 => dx.abs() + dy.abs(),
                        3 => dx.abs().max(dy.abs()),
                        4 => {
                            let a = ((dx.abs() as f32 / 4096.).sqrt() * 4096.) as i32;
                            let b = ((dy.abs() as f32 / 4096.).sqrt() * 4096.) as i32;
                            ((a + b) * (a + b)) >> 12
                        }
                        5 => {
                            (((dx * dx + dy * dy) as f32 / 16777216.).sqrt().sqrt() * 4096.) as i32
                        }
                        _ => (((dx * dx + dy * dy) as f32 / 16777216.).sqrt() * 4096.) as i32,
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
                0 => near[0],
                1 => near[1],
                2 => near[1] - near[0],
                3 => near[2],
                4 => near[3],
                n => bail!("Unknown Voronoi mode {n}"),
            };
            out[y * n + x] = [v; 3];
        }
    }
    Ok(())
}
fn emboss(op: &Op, input: &Image, out: &mut Image, n: usize) {
    let az = op.v(1, 3216) as f32 / 4096.;
    let el = op.v(2, 3216) as f32 / 4096.;
    let mut light = [
        (4096. * el.cos() * az.sin()) as i32,
        (4096. * el.cos() * az.cos()) as i32,
        (4096. * el.sin()) as i32,
    ];
    let s = ((((light[0] * light[0]) >> 12)
        + ((light[1] * light[1]) >> 12)
        + ((light[2] * light[2]) >> 12))
        >> 12) as f64;
    let scale = (s.sqrt() * 4096.) as i32;
    if scale != 0 {
        for v in &mut light {
            *v = (*v << 12) / scale;
        }
    }
    let mult = (op.v(0, 4096) * (n as i32 * 32)) >> 12;
    for y in 0..n {
        for x in 0..n {
            let dx = (mult
                * (input[y * n + wrap(x as i32 - 1, n)][0]
                    - input[y * n + wrap(x as i32 + 1, n)][0]))
                >> 12;
            let dy = (mult
                * (input[wrap(y as i32 + 1, n) * n + x][0]
                    - input[wrap(y as i32 - 1, n) * n + x][0]))
                >> 12;
            let a = (dx >> 4).abs().min(255);
            let b = (dy >> 4).abs().min(255);
            let inv = (255. / ((a * a + b * b + 65535) as f32 / 65535.).sqrt()) as i32;
            let value = ((light[0] * ((inv * dx) >> 8)) >> 12)
                + ((light[1] * ((inv * dy) >> 8)) >> 12)
                + ((light[2] * ((inv * 4096) >> 8)) >> 12);
            out[y * n + x] = [value; 3];
        }
    }
}
fn bricks(op: &Op, out: &mut Image, n: usize) -> Result<()> {
    let cols = op.v(0, 4) as usize;
    let rows = op.v(1, 8) as usize;
    ensure!(cols > 0 && rows > 0, "Invalid brick grid");
    let mut rng = Random::new(rows as i32);
    let w = 4096 / cols as i32;
    let h = 4096 / rows as i32;
    let mut ys = vec![0; rows + 1];
    let mut xs = vec![vec![0; cols + 1]; rows];
    let mut values = vec![vec![0; cols]; rows];
    for row in 0..rows {
        if row > 0 {
            ys[row] = ys[row - 1]
                + h
                + (((((rng.next(4096)? - 2048) * op.v(3, 204)) >> 12) * (h / 2)) >> 12);
        }
        for col in 0..cols {
            if col > 0 {
                xs[row][col] = xs[row][col - 1]
                    + w
                    + (((((rng.next(4096)? - 2048) * op.v(2, 409)) >> 12) * (w / 2)) >> 12);
            }
            values[row][col] = if op.v(7, 1024) > 0 {
                4096 - rng.next(op.v(7, 1024))?
            } else {
                4096
            };
        }
        xs[row][cols] = 4096;
    }
    ys[rows] = 4096;
    let margin = op.v(6, 81) / 2;
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
                0
            };
            out[y * n + x] = [v; 3];
        }
    }
    Ok(())
}
fn lines(op: &Op, out: &mut Image, n: usize) -> Result<()> {
    let mut rng = Random::new(op.v(0, 0));
    let spread = op.v(4, 4096);
    for _ in 0..op.v(1, 2000) {
        let a = if spread > 0 {
            op.v(3, 0) - spread / 2 + rng.next(spread)?
        } else {
            op.v(3, 0)
        };
        let angle = ((a >> 4) & 255) as f64 / 255. * std::f64::consts::TAU;
        let (mut x0, mut y0) = (rng.next(n as i32)?, rng.next(n as i32)?);
        let (mut x1, mut y1) = (
            x0 + (((angle.cos() * 4096.) as i32 * op.v(2, 16)) >> 12),
            y0 + (((angle.sin() * 4096.) as i32 * op.v(2, 16)) >> 12),
        );
        let steep = (x1 - x0).abs() < (y1 - y0).abs();
        if x0 == x1 && y0 == y1 {
            continue;
        }
        if steep {
            std::mem::swap(&mut x0, &mut y0);
            std::mem::swap(&mut x1, &mut y1);
        }
        if x0 > x1 {
            std::mem::swap(&mut x0, &mut x1);
            std::mem::swap(&mut y0, &mut y1);
        }
        let dx = x1 - x0;
        let dy = (y1 - y0).abs();
        let mut error = -dx / 2;
        let step = if y1 <= y0 { -1 } else { 1 };
        let slope = 2048 / dx;
        let base = 2048 - (rng.next(4096)? >> 2);
        let mut yy = y0;
        for x in x0..x1 {
            error += dy;
            let value = slope * (x - x0) + base;
            let line = wrap(yy, n);
            if error > 0 {
                yy += step;
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
        let mut small = vec![[0; 3]; 64 * 64];
        let mut large = vec![[0; 3]; 128 * 128];
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
    fn java_random_sequence() {
        let mut r = Random::new(0);
        assert_eq!(r.bits(), 3139482720);
        assert_eq!(r.bits(), 3571011896);
    }
    #[test]
    fn linear_curve() {
        let o = Op {
            kind: 8,
            fields: BTreeMap::new(),
            inputs: vec![],
            mono: true,
        };
        let c = curve(&o).unwrap();
        assert_eq!(c[0], 0);
        assert_eq!(c[128], 2048);
        assert_eq!(c[256], 4096);
    }
}
