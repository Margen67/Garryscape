// SPDX-License-Identifier: AGPL-3.0-only
pub mod dmx;
pub mod gltf;
pub mod vtf;
use crate::region::RegionMesh;
use anyhow::Result;
use std::{fs, path::Path};
pub fn png(baked: &RegionMesh) -> Result<Vec<u8>> {
    let mut output = vec![];
    {
        let mut enc = png::Encoder::new(&mut output, baked.width as u32, baked.height as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header()?.write_image_data(&baked.pixels)?;
    }
    Ok(output)
}
pub fn write(baked: &RegionMesh, root: &Path, namespace: &str, id: &str) -> Result<()> {
    let source = root
        .join("source")
        .join("garryscape")
        .join(namespace)
        .join("regions")
        .join(id);
    let materials = root
        .join("game/materials/models/garryscape")
        .join(namespace)
        .join("regions");
    let preview = root.join("preview");
    for p in [&source, &materials, &preview] {
        fs::create_dir_all(p)?;
    }
    let png = png(baked)?;
    gltf::write(baked, &png, &preview, id)?;
    vtf::write(baked, &materials.join(format!("{id}.vtf")))?;
    for (i, surface) in baked.materials.iter().enumerate() {
        let [x, y, w, h] = surface.rect;
        let alpha = if surface.water {
            "\"$translucent\" \"1\"\n"
        } else {
            ""
        };
        let shader = if surface.water {
            "GarryscapeAtlasWater"
        } else {
            "GarryscapeAtlas"
        };
        let t = surface.tint;
        let [cx, cy, cs, _] = baked.blending.rect;
        let [m0, m1, m2, m3] = baked.mapping;
        fs::write(materials.join(format!("{id}_{i}.vmt")), format!(
            "\"VertexLitGeneric\"\n{{\n\"EGSMShader\" \"{shader}\"\n\"$basetexture\" \"models/garryscape/{namespace}/regions/{id}\"\n\"$atlasrect\" \"[{x} {y} {w} {h}]\"\n\"$atlasinfo\" \"[{aw} {w} {h} {mip}]\"\n\"$atlasdimensions\" \"[{aw} {ah} 0 0]\"\n\"$tint\" \"[{r} {g} {b} {a}]\"\n\"$blendcontrol\" \"[{cx} {cy} {cs} 0]\"\n\"$regionmapping\" \"[{m0} {m1} {m2} {m3}]\"\n\"$texturescale\" \"[{scale} 0 0 0]\"\n{alpha}}}\n",
            aw=baked.width, ah=baked.height, mip=if w>0 {w.ilog2()}else{0},
            r=t[0],g=t[1],b=t[2],a=t[3],scale=surface.scale))?;
    }
    fs::write(
        preview.join("open_in_blender.py"),
        include_str!("../../../tools/preview_atlas.py"),
    )?;
    fs::write(
        preview.join("ground_blending.json"),
        serde_json::to_vec(&serde_json::json!({
            "blending":baked.blending,"mapping":baked.mapping,
        "floors":baked.blending.floors.iter().map(|(&id,f)| (id.to_string(),serde_json::json!({
            "rect":baked.atlas_images.iter().find(|i|i.source_ids.contains(&f.texture)).map_or([0;4],|i|i.rect),
            "tint":baked.blending.tints[&id],"texture":f.texture,"scale":f.scale
        }))).collect::<std::collections::BTreeMap<_,_>>(),
            "triangles":baked.triangles.iter().map(|t|t.layer).collect::<Vec<_>>()
        }))?,
    )?;
    dmx::write(baked, &source, namespace, id)?;
    fs::write(root.join("game/gameinfo.txt"),"\"GameInfo\"\n{\n\"game\" \"Garryscape asset build\"\n\"FileSystem\"\n{\n\"SteamAppId\" \"4000\"\n\"SearchPaths\"\n{\n\"game\" \"|gameinfo_path|.\"\n}\n}\n}\n")?;
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema":4,"region":id,"namespace":namespace,"triangles":baked.triangles.len(),"trianglesPerFloor":baked.plane_triangles,
            "tiles":baked.tiles,"uniqueImages":baked.unique_tiles,"atlasWidth":baked.width,"atlasHeight":baked.height,"textureCount":1,"collision":false,"padding":0,"resizedImages":0,
            "materialCount":baked.materials.len(),"materials":baked.materials,"atlasImages":baked.atlas_images,
            "shaderModule":"EGSM","groundBlending":baked.blending,"regionMapping":baked.mapping,"underwaterTriangles":baked.underwater_triangles,
            "nativeSurfaceTriangles":baked.triangles.iter().filter(|t|!t.underwater).count(),
            "waterTriangles":baked.triangles.iter().filter(|t|t.water).count(),
            "sourceBounds":baked.bounds,"model":format!("models/garryscape/{namespace}/regions/{id}.mdl"),
            "previewCameraLocal":[0.,0.,baked.bounds[1][2]+1200.],"previewPitch":89,
        }))?,
    )?;
    println!("glTF: {}", preview.join(format!("{id}.gltf")).display());
    println!("QC: {}", source.join("model.qc").display());
    Ok(())
}
