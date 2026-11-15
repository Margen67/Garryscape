// SPDX-License-Identifier: AGPL-3.0-only
use crate::bake::Baked;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};
fn pad(v: &mut Vec<u8>, byte: u8) {
    while !v.len().is_multiple_of(4) {
        v.push(byte);
    }
}
fn view(bin: &mut Vec<u8>, views: &mut Vec<Value>, data: &[u8], target: Option<u32>) -> usize {
    pad(bin, 0);
    let offset = bin.len();
    bin.extend_from_slice(data);
    let mut v = json!({"buffer":0,"byteOffset":offset,"byteLength":data.len()});
    if let Some(target) = target {
        v["target"] = json!(target);
    }
    views.push(v);
    views.len() - 1
}
fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}
pub fn write(baked: &Baked, png: &[u8], path: &Path) -> Result<()> {
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
        json!({"bufferView":position,"componentType":5126,"count":vertices.len(),"type":"VEC3","min":min,"max":max}),
        json!({"bufferView":normal,"componentType":5126,"count":vertices.len(),"type":"VEC3"}),
        json!({"bufferView":uv,"componentType":5126,"count":vertices.len(),"type":"VEC2"}),
    ];
    let mut primitives = vec![];
    let mut materials = vec![];
    for water in [false, true] {
        let indices = baked
            .triangles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.water == water)
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
            json!({"bufferView":index,"componentType":5125,"count":indices.len(),"type":"SCALAR"}),
        );
        let material = materials.len();
        materials.push(json!({"name":if water {"water"}else{"terrain"},"pbrMetallicRoughness":{"baseColorTexture":{"index":0},"metallicFactor":0,"roughnessFactor":1},"alphaMode":if water {"BLEND"}else{"OPAQUE"},"doubleSided":false}));
        primitives.push(json!({"attributes":{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2},"indices":accessor,"material":material,"mode":4}));
    }
    pad(&mut bin, 0);
    fs::create_dir_all(path)?;
    let id = path
        .file_name()
        .and_then(|s| s.to_str())
        .context("Invalid glTF directory name")?;
    let buffer_name = format!("{id}.bin");
    let image_name = format!("{id}.png");
    let document = json!({"asset":{"version":"2.0","generator":"Garryscape offline converter"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"terrain","mesh":0}],"meshes":[{"name":"region","primitives":primitives}],"materials":materials,"textures":[{"sampler":0,"source":0}],"samplers":[{"magFilter":9729,"minFilter":9987,"wrapS":33071,"wrapT":33071}],"images":[{"uri":image_name}],"buffers":[{"uri":buffer_name,"byteLength":bin.len()}],"bufferViews":views,"accessors":accessors});
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
mod tests {
    use super::*;
    use crate::bake::{Triangle, Vertex};
    #[test]
    fn external_texture_is_written_verbatim_at_original_dimensions() {
        let positions = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let vertices = positions.map(|pos| Vertex {
            pos,
            normal: [0., 0., 1.],
            uv: [0., 0.],
        });
        let baked = Baked {
            triangles: vec![Triangle {
                vertices,
                water: false,
            }],
            pixels: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 255,
            ],
            size: 2,
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
        write(&baked, &encoded, &directory).unwrap();
        assert_eq!(fs::read(directory.join("fixture.png")).unwrap(), encoded);
        let decoder = png::Decoder::new(std::io::Cursor::new(&encoded));
        let mut reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (2, 2));
        let mut decoded = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut decoded).unwrap();
        assert_eq!(decoded, baked.pixels);
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
