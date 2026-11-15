# Garryscape

Offline **2009Scape (RuneScape revision 530)** terrain conversion for Garry's Mod.
The Rust converter reads the native cache and exports a Blender-preview **GLB**,
**DMX**, QC, and Source materials. Git Bash invokes Garry's Mod's StudioMDL to
produce one region model. **No SMD and no cache decoding inside GMod.**

The current milestone is **region 50/50, terrain only, all four floors, without
collision**. Objects, NPCs, streaming, and gameplay come later. This is a rebuilt
converter, not the former lost release; see [RECOVERY.md](RECOVERY.md).

## Windows / Git Bash

Install [Rust](https://rustup.rs/) (the MSVC toolchain also needs Visual Studio's
C++ build tools and Windows SDK), Garry's Mod, and optionally Blender. The script
works from any working directory, including paths containing spaces. Run:

```bash
./convert.sh regions 50 50
```

Defaults:

| Setting | Path/value |
| --- | --- |
| Cache | `${UserProfile}/2009scape/cache/runescape` (`USERPROFILE` also accepted) |
| Scale | `0.68` |
| Namespace | `2009scape` |
| StudioMDL | `C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/bin/win64/studiomdl.exe` |
| Addon | `C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/addons/garryscape` |
| Working output | `output/` beside `convert.sh` |

The addon default follows the requested path exactly. If your installation loads
addons from `GarrysMod/garrysmod/addons`, supply that destination with `--addon`.
The script does not edit game search paths or download a cache.

The cache must contain `main_file_cache.dat2` and its `main_file_cache.idx*` files
(a `LIVE/` subdirectory and `.dat` suffixes on index files are also accepted).
It remains read-only; every consumed group is checked against its reference CRC.
The 2009Scape Server repository is not needed for this milestone.

```bash
# Export for Blender, without running StudioMDL or installing anything:
./convert.sh regions 50 50 --preview-only

# Explicit paths and scale:
./convert.sh regions 50 50 --cache 'C:/Users/marge/2009scape/cache/runescape' --scale 0.68

# Use revision number 530 instead of an RSPS namespace:
./convert.sh regions 50 50 --name ''

# Override the installed addon location:
./convert.sh regions 50 50 --addon 'D:/Steam/GarrysMod/garrysmod/addons/garryscape'
```

Coordinates are mandatory. `--help` lists all options. The Rust CLI also supports
`inspect-cache PATH`. Other regions may be requested, but 50/50 is the validation
target; an unsupported procedural texture operation produces an explicit error
rather than silently substituting a texture.

## Output layout

For `regions 50 50`, namespace `2009scape`:

```text
output/build/2009scape/regions/50_50/
  preview/garryscape/2009scape/regions/50_50.glb
  source/garryscape/2009scape/regions/50_50/
    model.qc
    reference_0.dmx ... reference_4.dmx
    compile.log
  game/
    gameinfo.txt
    models/garryscape/2009scape/regions/50_50.{mdl,vvd,dx90.vtx,...}
    materials/models/garryscape/2009scape/regions/
      50_50.vtf
      50_50.vmt
      50_50_water.vmt
  manifest.json
  cache-crcs.json
  complete.json
  compiled.json
  compiled-tool.txt
```

The addon receives **only** model companions, materials, the small server Lua
loader, license, and `data/garryscape/terrain_preview.json`. GLB, DMX, QC, logs, cache files,
and the working `game/` stay outside the addon. Generated assets are ignored by
Git; commits contain source, scripts, tests, documentation, and notices only.

The several single-mesh DMX inputs are QC bodies in **one final `.mdl`**, not
separate region entities. They keep each importer input below 7,200 vertices.
The format is Source's `model 1` / `keyvalues2 1` DMX.

All floor textures and transitions are baked offline into **one texture** with
128×128 source samples and 16-pixel extruded gutters. Opaque ground and translucent water
use two stock `VertexLitGeneric` materials referencing that same VTF. Source
handles lighting and culling; Lua performs no per-frame rendering or baking.
The GLB embeds the same atlas, has one mesh, and uses metres/Y-up; DMX uses
Source inches/Z-up. Both incorporate `--scale`.

The full four-floor 50/50 bake has 10,733 triangles. Its lossless RGBA8888
8192×8192 VTF, including mipmaps, is approximately **341 MiB**. This is the memory
tradeoff of retaining original samples in one texture. Textures are not silently
resized to fit; exports exceeding an 8192px atlas fail explicitly.

Water is a static bake of the cache texture with transparency, not a recreation
of RT4's animated/refraction HD water shader. Cache environmental lights are
parsed, but not reproduced as dynamic Source lights. Stock Source lighting will
therefore differ from RuneScape's lighting.

Unchanged input/configuration reuses converted files and the compiled model.
Changing the StudioMDL executable triggers compilation again; `--rebuild` forces
both conversion and compilation.
The fingerprint includes converter executable contents, cache-file timestamps
and sizes, region, namespace, and scale. Use `--rebuild` after manually replacing
cache data while preserving its timestamps, or to regenerate deliberately.
Failed StudioMDL inputs and logs are retained, and a failed build is not installed.

## Map preview

On **`garryscape.bsp` only**, the server Lua loader creates a native, non-solid
`prop_dynamic_override` at **`0 0 -12000`**. It enables noclip and positions players
**1,200 units above the exported terrain bounds**, looking down at **`89 0 0`**,
on spawn. This follows the subsequent request to put the camera above the terrain
instead of the originally specified `0 0 -13200`. The terrain also respawns after
map cleanup. Other maps are unaffected.

## Validation

```bash
cargo test --locked --manifest-path converter/Cargo.toml
cargo clippy --locked --manifest-path converter/Cargo.toml --all-targets -- -D warnings
bash -n convert.sh
./convert.sh regions 50 50 --cache /path/to/cache --preview-only
python tests/validate_region.py output/build/2009scape/regions/50_50
python tests/test_script.py
```

For visual inspection, import the GLB into Blender; Blender is not required to
convert or compile. Linux can export the inputs using `--preview-only`; actual
Windows StudioMDL and in-game rendering must be checked on Windows. Automated
script tests use a fake compiler to verify orchestration, not to claim a real
MDL compilation.

## References

- [2009Scape RT4 client](https://gitlab.com/2009scape/rt4-client): revision-530 map,
  floor, procedural texture and material behavior.
- [2009Scape server](https://gitlab.com/2009scape/2009scape): reference for later
  gameplay milestones, not required at runtime now.
- [rs-map-viewer](https://github.com/dennisdev/rs-map-viewer): shaped tile tables
  and procedural texture algorithms. Adapted portions retain its BSD-2-Clause
  notice in [third_party/rs-map-viewer-LICENSE](third_party/rs-map-viewer-LICENSE).

## License

The converter and Lua addon source are licensed under the GNU Affero General
Public License, version 3 ([AGPL-3.0-only](LICENSE)). Adapted rs-map-viewer code
retains its BSD-2-Clause notice in `third_party/rs-map-viewer-LICENSE`.
Game/cache assets are not included or relicensed by this repository.
