// SPDX-License-Identifier: AGPL-3.0-only
use crate::region::RegionMesh;
use anyhow::Result;
use serde_json::{json, Value};
use std::{fs, path::Path};
/// Align a binary glTF buffer to a four-byte boundary.
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
fn pad(v: &mut Vec<u8>, byte: u8) {
    while !v.len().is_multiple_of(4) {
        v.push(byte);
    }
}
/// Append binary data and its buffer-view descriptor.
#[allow(
    clippy::indexing_slicing,
    reason = "indices follow validated native tables and allocated image dimensions"
)]
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::shadow_reuse,
    reason = "local rebinding records successive numeric representations"
)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
fn view(bin: &mut Vec<u8>, views: &mut Vec<Value>, data: &[u8], target: Option<u32>) -> usize {
    pad(bin, 0);
    let offset = bin.len();
    bin.extend_from_slice(data);
    let mut v = json!({"buffer":0_i32,"byteOffset":offset,"byteLength":data.len()});
    if let Some(target) = target {
        v["target"] = json!(target);
    }
    views.push(v);
    views.len() - 1
}
/// Encode float components as little-endian glTF bytes.
#[allow(
    clippy::little_endian_bytes,
    reason = "the output file format specifies little-endian byte order"
)]
fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}
/// Write the glTF preview, binary buffer and unmodified atlas PNG.
#[allow(
    clippy::too_many_lines,
    reason = "keep native opcode dispatch and ordered conversion stages together"
)]
#[allow(
    clippy::little_endian_bytes,
    reason = "the output file format specifies little-endian byte order"
)]
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::float_arithmetic,
    clippy::suboptimal_flops,
    reason = "preserve native fixed-point truncation, bounded coordinate math and separate floating-point rounding"
)]
pub(crate) fn write(baked: &RegionMesh, png: &[u8], path: &Path, id: &str) -> Result<()> {
    let mut bin = vec![];
    let mut views = vec![];
    let vertices = baked
        .triangles
        .iter()
        .flat_map(|t| t.vertices)
        .collect::<Vec<_>>();
    // Source uses inches and Z up; glTF uses metres and Y up. Proper rotation,
    // without reflection, preserves winding and backface culling.
    let position = view(
        &mut bin,
        &mut views,
        &floats(
            vertices
                .iter()
                .flat_map(|v| [v.pos[0] * 0.0254, v.pos[2] * 0.0254, -v.pos[1] * 0.0254]),
        ),
        Some(34962),
    );
    let normal = view(
        &mut bin,
        &mut views,
        &floats(
            vertices
                .iter()
                .flat_map(|v| [v.normal[0], v.normal[2], -v.normal[1]]),
        ),
        Some(34962),
    );
    let uv = view(
        &mut bin,
        &mut views,
        &floats(vertices.iter().flat_map(|v| v.uv)),
        Some(34962),
    );
    // A separate preview-only UV channel carries local terrain coordinates and
    // layer. Original repeat UVs remain untouched in TEXCOORD_0.
    let terrain = view(
        &mut bin,
        &mut views,
        &floats(baked.triangles.iter().flat_map(|t| {
            t.vertices
                .map(|v| {
                    [
                        (v.pos[0] * baked.mapping[0] + 4096.) / 128.,
                        (v.pos[1] * baked.mapping[0] + 4096.) / 128. + f32::from(t.layer) * 128.,
                    ]
                })
                .into_iter()
                .flatten()
        })),
        Some(34962),
    );
    let min = [
        baked.bounds[0][0] * 0.0254,
        baked.bounds[0][2] * 0.0254,
        -baked.bounds[1][1] * 0.0254,
    ];
    let max = [
        baked.bounds[1][0] * 0.0254,
        baked.bounds[1][2] * 0.0254,
        -baked.bounds[0][1] * 0.0254,
    ];
    let mut accessors = vec![
        json!({"bufferView":position,"componentType":5_126_i32,"count":vertices.len(),"type":"VEC3","min":min,"max":max}),
        json!({"bufferView":normal,"componentType":5_126_i32,"count":vertices.len(),"type":"VEC3"}),
        json!({"bufferView":uv,"componentType":5_126_i32,"count":vertices.len(),"type":"VEC2"}),
    ];
    accessors.push(
        json!({"bufferView":terrain,"componentType":5_126_i32,"count":vertices.len(),"type":"VEC2"}),
    );
    let mut primitives = vec![];
    let mut materials = vec![];
    for (material, surface) in baked.materials.iter().enumerate() {
        let water = surface.water;
        let indices = baked
            .triangles
            .iter()
            .enumerate()
            .filter(|&(_, t)| t.material == material)
            .flat_map(|(i, _)| [i as u32 * 3, i as u32 * 3 + 1, i as u32 * 3 + 2])
            .collect::<Vec<_>>();
        if indices.is_empty() {
            continue;
        }
        let data = indices
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect::<Vec<_>>();
        let index = view(&mut bin, &mut views, &data, Some(34963));
        let accessor = accessors.len();
        accessors.push(
            json!({"bufferView":index,"componentType":5_125_i32,"count":indices.len(),"type":"SCALAR"}),
        );
        let pbr = if surface.texture >= 0_i32 {
            json!({"baseColorTexture":{"index":0_i32},"baseColorFactor":surface.tint,"metallicFactor":0_i32,"roughnessFactor":1_i32})
        } else {
            json!({"baseColorFactor":surface.tint,"metallicFactor":0_i32,"roughnessFactor":1_i32})
        };
        materials.push(json!({"name":format!("{id}_{material}"),"pbrMetallicRoughness":pbr,"alphaMode":if water {"BLEND"}else{"OPAQUE"},"doubleSided":false,
            "extras":{"garryscapeAtlas":{"rect":surface.rect,"atlasWidth":baked.width,"atlasHeight":baked.height,"nativeTexture":surface.texture,"requiresAtlasWrap":true,"binding":material}}}));
        primitives.push(json!({"attributes":{"POSITION":0_i32,"NORMAL":1_i32,"TEXCOORD_0":2_i32,"TEXCOORD_1":3_i32},"indices":accessor,"material":material,"mode":4_i32}));
    }
    pad(&mut bin, 0);
    fs::create_dir_all(path)?;
    let buffer_name = format!("{id}.bin");
    let image_name = format!("{id}.png");
    let document = json!({"asset":{"version":"2.0","generator":"Garryscape offline converter"},"scene":0_i32,"scenes":[{"nodes":[0_i32]}],"nodes":[{"name":"terrain","mesh":0_i32}],"meshes":[{"name":"region","primitives":primitives}],"materials":materials,"textures":[{"sampler":0_i32,"source":0_i32}],"samplers":[{"magFilter":9_729_i32,"minFilter":9_987_i32,"wrapS":33_071_i32,"wrapT":33_071_i32}],"images":[{"uri":image_name}],"buffers":[{"uri":buffer_name,"byteLength":bin.len()}],"bufferViews":views,"accessors":accessors});
    fs::write(path.join(&buffer_name), bin)?;
    // Write the original encoded atlas verbatim: no preview resize or recompression.
    fs::write(path.join(&image_name), png)?;
    fs::write(
        path.join(format!("{id}.gltf")),
        serde_json::to_vec_pretty(&document)?,
    )?;
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
    use crate::region::{Triangle, Vertex};
    #[test]
    #[allow(
        clippy::absolute_paths,
        reason = "qualified names identify the format or IO operation directly"
    )]
    fn external_texture_is_written_verbatim_at_original_dimensions() {
        let positions = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let vertices = positions.map(|pos| Vertex {
            pos,
            normal: [0., 0., 1.],
            uv: [0., 0.],
        });
        let baked = RegionMesh {
            triangles: vec![Triangle {
                vertices,
                water: false,
                material: 0,
                underwater: false,
                layer: 0,
                blend_address: 0,
            }],
            pixels: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 255,
            ],
            materials: vec![crate::region::Surface {
                rect: [0, 0, 2, 2],
                tint: [1.; 4],
                texture: 0,
                water: false,
                scale: 128.,
            }],
            atlas_images: vec![],
            blending: crate::blending::Blending::default(),
            mapping: [1., 4096., 0., 0.],
            underwater_triangles: 0,
            width: 2,
            height: 2,
            tiles: 1,
            unique_tiles: 1,
            bounds: [[0., 0., 0.], [1., 1., 0.]],
            plane_triangles: [1, 0, 0, 0],
        };
        let encoded = crate::export::png(&baked).unwrap();
        let directory = std::env::temp_dir()
            .join(format!(
                "garryscape-gltf-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("fixture");
        write(&baked, &encoded, &directory, "fixture").unwrap();
        assert_eq!(fs::read(directory.join("fixture.png")).unwrap(), encoded);
        let decoder = png::Decoder::new(std::io::Cursor::new(&encoded));
        let mut reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (2, 2));
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let _consumed = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(pixels, baked.pixels);
        let document: Value =
            serde_json::from_slice(&fs::read(directory.join("fixture.gltf")).unwrap()).unwrap();
        assert_eq!(document["images"][0]["uri"], "fixture.png");
        assert_eq!(document["buffers"][0]["uri"], "fixture.bin");
        assert!(document["images"][0].get("bufferView").is_none());
        assert_eq!(
            fs::metadata(directory.join("fixture.bin")).unwrap().len(),
            document["buffers"][0]["byteLength"].as_u64().unwrap()
        );
        fs::remove_dir_all(directory.parent().unwrap()).unwrap();
    }
}
