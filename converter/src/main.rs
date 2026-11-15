// SPDX-License-Identifier: AGPL-3.0-only
mod binary;
mod cache;
mod export;
mod region;
mod terrain;
mod textures;

use anyhow::{bail, ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
const HELP:&str="Garryscape offline converter\n\n  garryscape-convert regions <x> <y> [--cache PATH] [--output PATH]\n      [--scale 0.68] [--name 2009scape] [--rebuild]\n  garryscape-convert inspect-cache <native-cache-directory>\n\nOnly terrain is implemented. No objects or collision are exported.\nAn empty --name uses the revision number (530).";
fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    if matches!(command.as_deref(), None | Some("--help" | "-h")) {
        println!("{HELP}");
        return Ok(());
    }
    if command.as_deref() == Some("inspect-cache") {
        let root = args.next().context("Native cache directory required")?;
        ensure!(args.next().is_none(), "Unexpected inspect-cache argument");
        let mut cache = cache::Cache::new(Path::new(&root))?;
        let region = terrain::load(&mut cache, 50, 50)?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"mapGroups":cache.index(5)?.len(),"lumbridgeFound":region.is_some(),"underwaterFound":region.as_ref().is_some_and(|r|r.underwater.is_some()),"verifiedGroups":cache.verified})
            )?
        );
        return Ok(());
    }
    ensure!(
        command.as_deref() == Some("regions"),
        "Only regions are implemented in this milestone.\n{HELP}"
    );
    let x: u8 = args
        .next()
        .context("Region X is required")?
        .parse()
        .context("Region X must be 0..255")?;
    let y: u8 = args
        .next()
        .context("Region Y is required")?
        .parse()
        .context("Region Y must be 0..255")?;
    let profile = std::env::var_os("UserProfile").or_else(|| std::env::var_os("USERPROFILE"));
    let mut cache_path = profile.map(|p| PathBuf::from(p).join("2009scape/cache/runescape"));
    let mut output = PathBuf::from("output");
    let mut scale = 0.68f32;
    let mut name = "2009scape".to_owned();
    let mut rebuild = false;
    while let Some(arg) = args.next() {
        if arg == "--rebuild" {
            rebuild = true;
            continue;
        }
        let value = args
            .next()
            .with_context(|| format!("{arg} requires a value"))?;
        match arg.as_str() {
            "--cache" => cache_path = Some(value.into()),
            "--output" => output = value.into(),
            "--scale" => scale = value.parse().context("Invalid scale")?,
            "--name" => name = value,
            _ => bail!("Unknown argument {arg}"),
        }
    }
    ensure!(
        scale.is_finite() && scale > 0. && scale <= 100.,
        "Scale must be finite and greater than 0 (maximum 100)"
    );
    if name.is_empty() {
        name = "530".into();
    }
    ensure!(
        name.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-'),
        "Name must contain only lowercase ASCII letters, digits, '-' or '_'"
    );
    let cache_path = cache_path.context("Set --cache or the UserProfile environment variable")?;
    let mut cache=cache::Cache::new(&cache_path).context("2009Scape cache not found; supply your revision-530 cache with --cache. No cache is downloaded.")?;
    let id = format!("{x}_{y}");
    let build = output.join("build").join(&name).join("regions").join(&id);
    fs::create_dir_all(&build)?;
    let fingerprint = fingerprint(&cache_path, scale, &name, x, y)?;
    let marker = build.join("complete.json");
    if !rebuild {
        if let Ok(bytes) = fs::read(&marker) {
            if let Ok(saved) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if saved["fingerprint"].as_u64() == Some(fingerprint as u64)
                    && outputs_present(&build, &name, &id)?
                {
                    println!("Using converted region {x}/{y}: {}", build.display());
                    return Ok(());
                }
            }
        }
    }
    if marker.exists() {
        fs::remove_file(&marker)?;
    }
    eprintln!("Decoding 2009Scape region {x}/{y}; scale {scale}");
    let world = terrain::World::load(&mut cache, x as i32, y as i32)?;
    let baked = region::build(&mut cache, &world, scale)?;
    export::write(&baked, &build, &name, &id)?;
    fs::write(
        build.join("cache-crcs.json"),
        serde_json::to_vec_pretty(&cache.verified)?,
    )?;
    fs::write(
        &marker,
        serde_json::to_vec_pretty(
            &serde_json::json!({"fingerprint":fingerprint,"version":env!("CARGO_PKG_VERSION"),"scale":scale,"revision":530,"cacheGroupsVerified":cache.verified.len()}),
        )?,
    )?;
    println!(
        "Exported terrain {x}/{y}: one model, one {}px texture, {} triangles; no collision",
        baked.size,
        baked.triangles.len()
    );
    Ok(())
}
fn outputs_present(build: &Path, name: &str, id: &str) -> Result<bool> {
    for path in [
        build.join(format!("preview/{id}.gltf")),
        build.join(format!("preview/{id}.bin")),
        build.join(format!("preview/{id}.png")),
        build.join("preview/open_in_blender.py"),
        build.join(format!("source/garryscape/{name}/regions/{id}/model.qc")),
        build.join(format!(
            "game/materials/models/garryscape/{name}/regions/{id}.vtf"
        )),
        build.join("manifest.json"),
    ] {
        if !path.is_file() {
            return Ok(false);
        }
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(build.join("manifest.json"))?)?;
    for i in 0..manifest["materialCount"].as_u64().unwrap_or(0) {
        if !build
            .join(format!(
                "game/materials/models/garryscape/{name}/regions/{id}_{i}.vmt"
            ))
            .is_file()
        {
            return Ok(false);
        }
    }
    let source = build.join(format!("source/garryscape/{name}/regions/{id}"));
    let qc = fs::read_to_string(source.join("model.qc"))?;
    for line in qc.lines().filter(|l| l.starts_with("$body ")) {
        let file = line.split('"').nth(1).context("Invalid cached QC")?;
        if !source.join(file).is_file() {
            return Ok(false);
        }
    }
    Ok(true)
}
fn fingerprint(root: &Path, scale: f32, name: &str, x: u8, y: u8) -> Result<u32> {
    let root = if root.join("main_file_cache.dat2").is_file()
        || root.join("main_file_cache.dat2.dat").is_file()
    {
        root.to_owned()
    } else {
        root.join("LIVE")
    };
    let mut files = fs::read_dir(&root)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    files.sort();
    let mut hash = crc32fast::Hasher::new();
    hash.update(env!("CARGO_PKG_VERSION").as_bytes());
    hash.update(&fs::read(std::env::current_exe()?)?);
    hash.update(&scale.to_bits().to_le_bytes());
    hash.update(name.as_bytes());
    hash.update(&[x, y]);
    for file in files {
        let filename = file.file_name().unwrap().to_string_lossy();
        if !filename.starts_with("main_file_cache.") {
            continue;
        }
        let metadata = fs::metadata(&file)?;
        hash.update(filename.as_bytes());
        hash.update(&metadata.len().to_le_bytes());
        hash.update(
            &metadata
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_nanos()
                .to_le_bytes(),
        );
    }
    Ok(hash.finalize())
}
