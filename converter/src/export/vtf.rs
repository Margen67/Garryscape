//! Lossless RGBA8888 VTF 7.2. Full mip chain, generated once offline.
use crate::bake::Baked;
use anyhow::Result;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};
pub fn write(baked: &Baked, path: &Path) -> Result<()> {
    let mut levels = vec![];
    let mut size = baked.size;
    let mut previous = &baked.pixels[..];
    while size > 1 {
        let next = downsample(previous, size);
        levels.push(next);
        size /= 2;
        previous = levels.last().unwrap();
    }
    let mut header = [0u8; 80];
    header[..4].copy_from_slice(b"VTF\0");
    header[4..8].copy_from_slice(&7u32.to_le_bytes());
    header[8..12].copy_from_slice(&2u32.to_le_bytes());
    header[12..16].copy_from_slice(&80u32.to_le_bytes());
    header[16..18].copy_from_slice(&(baked.size as u16).to_le_bytes());
    header[18..20].copy_from_slice(&(baked.size as u16).to_le_bytes());
    header[20..24].copy_from_slice(&(0x2000u32 | 8 | 4 | 2).to_le_bytes());
    header[24..26].copy_from_slice(&1u16.to_le_bytes());
    header[48..52].copy_from_slice(&1f32.to_le_bytes());
    header[52..56].copy_from_slice(&0u32.to_le_bytes());
    header[56] = (levels.len() + 1) as u8;
    header[57..61].copy_from_slice(&u32::MAX.to_le_bytes());
    header[63..65].copy_from_slice(&1u16.to_le_bytes());
    let mut output = BufWriter::new(File::create(path)?);
    output.write_all(&header)?;
    for level in levels.iter().rev() {
        output.write_all(level)?;
    }
    output.write_all(&baked.pixels)?;
    output.flush()?;
    Ok(())
}
fn downsample(input: &[u8], size: usize) -> Vec<u8> {
    let next = size / 2;
    let mut output = vec![0; next * next * 4];
    let linear: Vec<f32> = (0..256).map(|v| (v as f32 / 255.).powf(2.2)).collect();
    for y in 0..next {
        for x in 0..next {
            let mut rgb = [0.; 3];
            let mut alpha = 0.;
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = ((y * 2 + dy) * size + x * 2 + dx) * 4;
                    let a = input[i + 3] as f32 / 255.;
                    alpha += a;
                    for c in 0..3 {
                        rgb[c] += linear[input[i + c] as usize] * a;
                    }
                }
            }
            let dst = (y * next + x) * 4;
            for c in 0..3 {
                output[dst + c] = if alpha > 0. {
                    (rgb[c] / alpha).powf(1. / 2.2).mul_add(255., 0.).round() as u8
                } else {
                    0
                };
            }
            output[dst + 3] = (alpha * 255. / 4.).round() as u8;
        }
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_mips_do_not_dark_fringe() {
        let pixels = [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(downsample(&pixels, 2), [255, 0, 0, 64]);
    }
}
