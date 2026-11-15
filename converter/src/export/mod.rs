pub mod dmx;
pub mod glb;
pub mod vtf;
use crate::bake::Baked;
use anyhow::{Context, Result};
use std::{fs, path::Path};
pub fn png(baked: &Baked) -> Result<Vec<u8>> {
    let mut output = vec![];
    {
        let mut enc = png::Encoder::new(&mut output, baked.size as u32, baked.size as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header()?.write_image_data(&baked.pixels)?;
    }
    Ok(output)
}
pub fn write(baked: &Baked, root: &Path, namespace: &str, id: &str) -> Result<()> {
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
    let preview = root
        .join("preview/garryscape")
        .join(namespace)
        .join("regions");
    for p in [&source, &materials, &preview] {
        fs::create_dir_all(p)?;
    }
    let png = png(baked)?;
    glb::write(baked, &png, &preview.join(format!("{id}.glb")))?;
    vtf::write(baked, &materials.join(format!("{id}.vtf")))?;
    for (suffix, water) in [("", false), ("_water", true)] {
        if water && !baked.triangles.iter().any(|t| t.water) {
            continue;
        }
        let alpha = if water {
            "\"$translucent\" \"1\"\n"
        } else {
            ""
        };
        fs::write(materials.join(format!("{id}{suffix}.vmt")),format!("\"VertexLitGeneric\"\n{{\n\"$basetexture\" \"models/garryscape/{namespace}/regions/{id}\"\n{alpha}}}\n"))?;
    }
    dmx::write(baked, &source, namespace, id)?;
    fs::write(root.join("game/gameinfo.txt"),"\"GameInfo\"\n{\n\"game\" \"Garryscape asset build\"\n\"FileSystem\"\n{\n\"SteamAppId\" \"4000\"\n\"SearchPaths\"\n{\n\"game\" \"|gameinfo_path|.\"\n}\n}\n}\n")?;
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema":1,"region":id,"namespace":namespace,"triangles":baked.triangles.len(),"trianglesPerFloor":baked.plane_triangles,
            "tiles":baked.tiles,"uniqueBakedTiles":baked.unique_tiles,"atlasSize":baked.size,"textureCount":1,"collision":false,
            "sourceBounds":baked.bounds,"model":format!("models/garryscape/{namespace}/regions/{id}.mdl"),
            "previewCameraLocal":[0.,0.,baked.bounds[1][2]+1200.],"previewPitch":89,
        }))?,
    )?;
    println!("GLB: {}", preview.join(format!("{id}.glb")).display());
    println!("QC: {}", source.join("model.qc").display());
    ensure_file(&source.join("model.qc"))?;
    Ok(())
}
fn ensure_file(path: &Path) -> Result<()> {
    fs::metadata(path).with_context(|| format!("Missing {}", path.display()))?;
    Ok(())
}
