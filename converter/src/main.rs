// SPDX-License-Identifier: AGPL-3.0-only
//! Offline revision-530 terrain conversion and Source build preparation.
// png uses miniz_oxide 0.8 and enables flate2 defaults, which pull in 0.9.
// flate2 selects zlib-rs when both Rust backends are enabled.
#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive png and flate2 version requirements"
)]
// Feature gates are necessary to check all nightly opt-in lints. These two
// diagnostics prohibit the very gates that activate those requested checks.
#![allow(
    unstable_features,
    internal_features,
    reason = "enable nightly lint checks"
)]
// Feature gates needed to activate the nightly-only compiler lint checks.
#![feature(
    link_llvm_intrinsics,
    marked_unchecked_type_aliases,
    multiple_supertrait_upcastable,
    must_not_suspend,
    non_exhaustive_omitted_patterns_lint,
    strict_provenance_lints,
    supertrait_item_shadowing,
    unqualified_local_imports
)]
/// Bounds-checked native cache primitive reader.
mod binary;
mod blending;
mod cache;
/// Offline preview and Source asset serializers.
mod export;
mod region;
mod terrain;
mod textures;

use anyhow::{bail, ensure, Context as _, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
/// Command-line usage and supported options.
const HELP:&str="Garryscape offline converter\n\n  garryscape-convert regions <x> <y> [--cache PATH] [--output PATH]\n      [--scale 0.68] [--name 2009scape] [--rebuild]\n  garryscape-convert inspect-cache <native-cache-directory>\n\nOnly terrain is implemented. No objects or collision are exported.\nAn empty --name uses the revision number (530).";
#[allow(
    clippy::print_stderr,
    reason = "the CLI reports errors and decoding progress to stderr"
)]
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}
/// Parse arguments and run a cached or fresh terrain conversion.
#[allow(
    clippy::too_many_lines,
    reason = "keep native opcode dispatch and ordered conversion stages together"
)]
#[allow(
    clippy::print_stdout,
    reason = "the CLI reports conversion results to stdout"
)]
#[allow(
    clippy::print_stderr,
    reason = "the CLI reports errors and decoding progress to stderr"
)]
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
#[allow(
    clippy::shadow_reuse,
    reason = "local rebinding records successive numeric representations"
)]
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
    let mut scale = 0.68_f32;
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
    let build = output.join(&name).join("regions").join(&id);
    fs::create_dir_all(&build)?;
    let fingerprint = fingerprint(&cache_path, scale, &name, x, y)?;
    let marker = build.join("complete.json");
    if !rebuild {
        if let Ok(bytes) = fs::read(&marker) {
            if let Ok(saved) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if saved.get("fingerprint").and_then(serde_json::Value::as_u64)
                    == Some(u64::from(fingerprint))
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
    let world = terrain::World::load(&mut cache, i32::from(x), i32::from(y))?;
    let baked = region::build(&mut cache, &world, scale)?;
    export::write(&baked, &build, &name, &id)?;
    fs::write(
        build.join("cache-crcs.json"),
        serde_json::to_vec_pretty(&cache.verified)?,
    )?;
    fs::write(
        &marker,
        serde_json::to_vec_pretty(
            &serde_json::json!({"fingerprint":fingerprint,"version":env!("CARGO_PKG_VERSION"),"scale":scale,"revision":530_i32,"cacheGroupsVerified":cache.verified.len()}),
        )?,
    )?;
    println!(
        "Exported terrain {x}/{y}: one model, one {}x{}px texture, {} triangles; no collision",
        baked.width,
        baked.height,
        baked.triangles.len()
    );
    Ok(())
}
/// Check every required preview and Source output before reusing a build.
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
fn outputs_present(build: &Path, name: &str, id: &str) -> Result<bool> {
    for path in [
        build.join(format!("preview/{id}.gltf")),
        build.join(format!("preview/{id}.bin")),
        build.join("preview-atlas.png"),
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
    if !build.join(format!("preview/{id}.png")).is_file()
        && !build.join(format!("preview/{id}.webp")).is_file()
    {
        return Ok(false);
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(build.join("manifest.json"))?)?;
    for i in 0..manifest
        .get("materialCount")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
    {
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
/// Hash cache metadata and conversion options for build reuse.
#[allow(
    clippy::little_endian_bytes,
    reason = "the output file format specifies little-endian byte order"
)]
#[allow(
    clippy::shadow_reuse,
    reason = "local rebinding records successive numeric representations"
)]
#[allow(
    clippy::absolute_paths,
    reason = "qualified names identify the format or IO operation directly"
)]
#[allow(
    clippy::min_ident_chars,
    reason = "retain coordinate and channel notation shared with the native reference algorithm"
)]
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
        let filename = file
            .file_name()
            .context("Cache entry has no filename")?
            .to_string_lossy();
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
