// SPDX-License-Identifier: AGPL-3.0-only
use crate::bake::Baked;
use anyhow::{ensure, Result};
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
    let image = view(&mut bin, &mut views, png, None);
    pad(&mut bin, 0);
    let document = json!({"asset":{"version":"2.0","generator":"Garryscape offline converter"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"terrain","mesh":0}],"meshes":[{"name":"region","primitives":primitives}],"materials":materials,"textures":[{"sampler":0,"source":0}],"samplers":[{"magFilter":9729,"minFilter":9987,"wrapS":33071,"wrapT":33071}],"images":[{"bufferView":image,"mimeType":"image/png"}],"buffers":[{"byteLength":bin.len()}],"bufferViews":views,"accessors":accessors});
    let mut json = serde_json::to_vec(&document)?;
    pad(&mut json, b' ');
    let length = 12 + 8 + json.len() + 8 + bin.len();
    ensure!(length <= u32::MAX as usize, "GLB too large");
    let mut output = Vec::with_capacity(length);
    output.extend_from_slice(b"glTF");
    output.extend_from_slice(&2u32.to_le_bytes());
    output.extend_from_slice(&(length as u32).to_le_bytes());
    output.extend_from_slice(&(json.len() as u32).to_le_bytes());
    output.extend_from_slice(b"JSON");
    output.extend(json);
    output.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    output.extend_from_slice(b"BIN\0");
    output.extend(bin);
    fs::write(path, output)?;
    Ok(())
}
