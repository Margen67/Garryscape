// SPDX-License-Identifier: AGPL-3.0-or-later
//! Lossless RGBA8888 VTF 7.5. Full mip chain, generated once offline.
use crate::region::RegionMesh;
use anyhow::{Context as _, Result};
use std::{
    fs::File,
    io::{BufWriter, Write as _},
    path::Path,
};
/// Write an RGBA8888 VTF with a complete alpha-aware mip chain.
#[allow(
    clippy::little_endian_bytes,
    reason = "the output file format specifies little-endian byte order"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
pub(crate) fn write(baked: &RegionMesh, path: &Path) -> Result<()> {
    let mut levels = vec![];
    let (mut width, mut height) = (baked.width, baked.height);
    let mut previous = &*baked.pixels;
    while width > 1 || height > 1 {
        let next = downsample(previous, width, height);
        levels.push(next);
        width = (width / 2).max(1);
        height = (height / 2).max(1);
        previous = levels.last().context("Missing generated mip level")?;
    }
    let mut header = [0_u8; 88];
    header[..4].copy_from_slice(b"VTF\0");
    header[4..8].copy_from_slice(&7_u32.to_le_bytes());
    header[8..12].copy_from_slice(&5_u32.to_le_bytes());
    header[12..16].copy_from_slice(&88_u32.to_le_bytes());
    header[16..18].copy_from_slice(&(baked.width as u16).to_le_bytes());
    header[18..20].copy_from_slice(&(baked.height as u16).to_le_bytes());
    // Control bytes share the albedo atlas. Texture-detail/picmip reduction
    // would destroy them; NOLOD protects the base level without disabling mips.
    let no_lod = if baked.blending.rect[2] > 0 { 0x200 } else { 0 };
    header[20..24].copy_from_slice(&(0x2000_u32 | 8 | 4 | 2 | no_lod).to_le_bytes());
    header[24..26].copy_from_slice(&1_u16.to_le_bytes());
    header[48..52].copy_from_slice(&1_f32.to_le_bytes());
    header[52..56].copy_from_slice(&0_u32.to_le_bytes());
    header[56] = (levels.len() + 1) as u8;
    header[57..61].copy_from_slice(&u32::MAX.to_le_bytes());
    header[63..65].copy_from_slice(&1_u16.to_le_bytes());
    // One resource: the high-resolution image, including all mip levels.
    header[68..72].copy_from_slice(&1_u32.to_le_bytes());
    header[80..84].copy_from_slice(&[0x30, 0, 0, 0]);
    header[84..88].copy_from_slice(&88_u32.to_le_bytes());
    let mut output = BufWriter::new(File::create(path)?);
    output.write_all(&header)?;
    for level in levels.iter().rev() {
        output.write_all(level)?;
    }
    output.write_all(&baked.pixels)?;
    output.flush()?;
    Ok(())
}
/// Generate the next VTF mip without resizing the base image.
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
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::float_arithmetic,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    clippy::suboptimal_flops,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn downsample(input: &[u8], width: usize, height: usize) -> Vec<u8> {
    let next_width = (width / 2).max(1);
    let next_height = (height / 2).max(1);
    let mut output = vec![0; next_width * next_height * 4];
    let linear: Vec<f32> = (0_i32..256_i32)
        .map(|v| (v as f32 / 255.).powf(2.2))
        .collect();
    for y in 0..next_height {
        for x in 0..next_width {
            let mut rgb = [0.; 3];
            let mut alpha = 0.;
            for dy in 0..2 {
                for dx in 0..2 {
                    let sx = (x * 2 + dx).min(width - 1);
                    let sy = (y * 2 + dy).min(height - 1);
                    let i = (sy * width + sx) * 4;
                    let a = f32::from(input[i + 3]) / 255.;
                    alpha += a;
                    for c in 0..3 {
                        rgb[c] += linear[input[i + c] as usize] * a;
                    }
                }
            }
            let dst = (y * next_width + x) * 4;
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
    #[allow(
        clippy::min_ident_chars,
        reason = "retain coordinate and channel notation shared with the native reference algorithm"
    )]
    #[allow(
        clippy::integer_division,
        clippy::integer_division_remainder_used,
        reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
    )]
    fn rectangular_mips_keep_the_last_row_or_column_until_one_by_one() {
        for (width, height) in [(8, 2), (2, 8), (1, 8), (8, 1)] {
            let mut pixels = vec![0_u8; width * height * 4];
            for p in pixels.as_chunks_mut::<4>().0 {
                p.copy_from_slice(&[12, 34, 56, 255]);
            }
            let (mut w, mut h) = (width, height);
            while w > 1 || h > 1 {
                pixels = downsample(&pixels, w, h);
                w = (w / 2).max(1);
                h = (h / 2).max(1);
                assert_eq!(pixels.len(), w * h * 4);
                assert!(pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| *p == [12, 34, 56, 255]));
            }
        }
    }
    #[test]
    fn alpha_mips_do_not_dark_fringe() {
        let pixels = [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(downsample(&pixels, 2, 2), [255, 0, 0, 64]);
    }
}
