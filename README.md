# Garryscape

Offline **2009Scape (RuneScape revision 530)** terrain conversion for Garry's Mod.
The Rust converter reads the native cache and exports a Blender-preview **glTF folder**,
**DMX**, QC, and Source materials. Git Bash invokes Garry's Mod's StudioMDL to
produce one region model. **No SMD and no cache decoding inside GMod.**

The current milestone is **region 50/50, terrain only, all four floors plus the underwater floor, without
collision**. Objects, NPCs, streaming, and gameplay come later. This is a rebuilt
converter, not the former lost release; see [RECOVERY.md](RECOVERY.md).

## Windows / Git Bash

Install [Rust through rustup](https://rustup.rs/) (the MSVC toolchain also needs
Visual Studio's C++ build tools and Windows SDK), Garry's Mod, and optionally
Blender. The repository pins **nightly-2026-11-19** with Clippy and rustfmt in
`rust-toolchain.toml`; rustup installs that toolchain on first use. The script
selects it from any working directory, including paths containing spaces. Run:

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
| Addon | `C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/garrysmod/addons/garryscape` |
| Working output | `output/` beside `convert.sh` |

The addon default is the installed game’s `garrysmod/addons/garryscape` directory.
Use `--addon` for a different Steam library. The script prints the destination
before compilation and does not edit game search paths or download a cache.

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
output/2009scape/regions/50_50/
  preview/
    50_50.gltf
    50_50.bin
    50_50.webp
    open_in_blender.py
    ground_blending.json
  source/garryscape/2009scape/regions/50_50/
    model.qc
    reference_0.dmx ... reference_4.dmx
    compile.log
  game/
    gameinfo.txt
    models/garryscape/2009scape/regions/50_50.{mdl,vvd,dx90.vtx,...}
    materials/models/garryscape/2009scape/regions/
      50_50.vtf
      50_50_0.vmt ... 50_50_22.vmt
  manifest.json
  cache-crcs.json
  complete.json
  compiled.json
  compiled-tool.txt
```

The addon receives **only** model companions, materials, the small server Lua
loader, license, and `data/garryscape/terrain_preview.json`. glTF, DMX, QC, logs, cache files,
and the working `game/` stay outside the addon. Generated assets are ignored by
Git; commits contain source, scripts, tests, documentation, and notices only.

The several single-mesh DMX inputs are QC bodies in **one final `.mdl`**, not
separate region entities. They keep each importer input below 7,200 vertices.
The format is Source's `model 1` / `keyvalues2 1` DMX.

The converter retains the cache's shaped terrain triangles and the original
texture-repeat scale. It loads both the four-floor surface map (`m50_50`) and
its separate underwater terrain (`um50_50`). Underwater shoreline vertices follow
RT4's per-vertex surface-height flags, so banks join the riverbed without a gap.
Shaped vertices use RT4's HD bilinear height interpolation and integer rounding. It does not simplify faces, add
replacement geometry, split triangles for UV seams, or rasterize terrain shapes
and transitions into per-tile images. The coordinate conversion changes axes,
uses a region-local origin, and applies `--scale`.

The single atlas contains native procedural texture images at their native
64×64 or 128×128 resolution. Only byte-identical RGBA images of the same
size are deduplicated. No image is enlarged, reduced, tinted, or given a border.
Unused outer atlas space is cropped to power-of-two width and height independently.
Transparent pixels inside each packed image are retained. Nothing is resized or
padded, and mesh geometry and native texture mapping remain unchanged. The shader and Blender
loader read per-region dimensions, including larger or rectangular atlases.
Base floor colour modulation remains a material parameter; blended layer tints
are stored with the interpolation controls. Normal VTF mipmaps retain the
full-resolution base level. `NOLOD` prevents texture-detail/picmip reduction from
corrupting the atlas control bytes; it does not disable mipmaps or the filtering
convars.

Region 50/50 has **10,733 surface triangles and 742 underwater triangles**.
The native-image atlas is **1024×512**, about **2.67 MiB** as lossless RGBA8888
with mipmaps, replacing the former 8192×8192 per-tile bake. The manifest records
native image rectangles and CRCs so pixel preservation can be checked.

The glTF preview requires `EXT_texture_webp`. The converter's built-in
encoder uses maximum lossless compression and preserves RGB under fully
transparent pixels. The Source VTF is produced separately.

## Required model shader: EGSM

Keeping one atlas while independently repeating each original image requires
an atlas-aware material shader. Garryscape includes its Lua/HLSL shader source,
using **[Akabenko's x86-64 EGSM module](https://github.com/Akabenko/EGSM)**.
GShader's post-processing library alone does not supply this native model shader.

Install EGSM using [its installation instructions](https://github.com/Akabenko/EGSM#how-to-install):
place the module in `garrysmod/lua/bin`, install its shader support files, and
load it from the menu realm as instructed by EGSM. Restart GMod afterward.
The converter installs Garryscape's shader source alongside the addon; it does
not download or distribute the external DLL or edit `menu.lua` automatically.

The shaders register once on the client. Native Source entities handle drawing;
there are no per-frame Lua model conversions or draw loops. The shader wraps
UVs within each native image, including bilinear/trilinear filtering at image
edges without neighboring atlas images bleeding in. Atlas-safe anisotropic
sampling follows `mat_forceaniso` (up to 16x; 0/1 uses isotropic trilinear
filtering). `mat_filtertextures 0` selects nearest texels, and
`mat_mipmaptextures 0` samples mip level zero. These settings also affect
textures sampled during blending. Changes update shader constants through convar callbacks, without
per-frame Lua polling or asset recompilation. Higher settings cost more GPU
texture samples. Source ambient lighting,
dynamic lights, and flashlight support use EGSM's standard Source constants.
The image is unchanged and the source UVs retain their original repeat frequency.

**Windows/EGSM GPU execution still needs in-game verification.** The module is
required for correct native-model atlas rendering; stock `VertexLitGeneric`
does not independently repeat packed images. Missing EGSM produces a diagnostic.
Ground blending uses RT4's underlay corner texture priorities, overlay blend
flags, rotated shape extensions, and neighboring edge/corner masks. The Rust
converter prepares compact, deduplicated triangle interpolation controls inside
the same atlas; it does not rasterize terrain or resize the native images.
The shader evaluates those controls only on blended triangles. Blender's loader
uses shared analytic mask nodes for the same transitions.

Toggle blending at **Utilities → Garryscape → Settings → Ground blending**, or
with `garryscape_ground_blending 0/1`. It defaults to enabled, is saved on the
client, and changes immediately without rebuilding. Source's single model UV
channel carries the precomputed blend address and terrain layer; the shader
recovers the native repeat coordinates from the unchanged local positions.
The glTF preview retains native UVs and adds a separate terrain-control channel.
No per-frame Lua calculation or cache decoding is needed.

HD water animation, depth fog and
the original RT4 lighting model are not
recreated by this atlas shader. Cache water RGBA is retained rather than replaced
with an invented alpha clamp. Underwater geometry is exported independently of
that remaining water-rendering work.

### Blender preview

Standard glTF material descriptions cannot express repeating subrectangles of
one atlas. The `.gltf` therefore carries native UVs and atlas metadata, and its
companion Blender loader builds the equivalent material nodes without changing
mesh vertices, triangles, or UVs:

```bash
# After conversion, open the preview in a new Blender process:
python output/2009scape/regions/50_50/preview/open_in_blender.py
```

The script looks for Blender on `PATH` and in Steam's default Windows install
location. You can also open the script in Blender's Text Editor and choose
**Run Script** to import into the current scene.

Use that loader for correct atlas wrapping. Plain glTF import preserves the
geometry and metadata but does not configure the custom atlas material. The
folder contains one lossless WebP atlas and one binary mesh buffer.

Unchanged input/configuration reuses converted files and the compiled model.
Changing the StudioMDL executable triggers compilation again; `--rebuild` forces
both conversion and compilation.
The fingerprint includes converter executable contents, cache-file timestamps
and sizes, region, namespace, and scale. Use `--rebuild` after manually replacing
cache data while preserving its timestamps, or to regenerate deliberately.
Failed StudioMDL inputs and logs are retained, and a failed build is not installed.
After a successful build (including reuse of a verified compiled model), the
script stages the addon, deletes the entire destination `garryscape` addon folder,
and replaces it with this build. Stale files and previously installed regions
are removed. Compilation or staging failures preserve the previous installation;
`--preview-only` never replaces it.

## Map preview

On **`garryscape.bsp` only**, the server Lua script enables noclip and teleports players on spawn
**1,200 units above the exported terrain bounds**, looking down at **`89 0 0`**.
It uses the installed manifest to calculate that position, with `0 0 -10400` as
its fallback. This follows the request to place the camera above the terrain
instead of the originally specified `0 0 -13200`.

Lua automatically spawns the compiled region model at **`0 0 -12000`** and
replaces it after map cleanup. The native Source entity handles rendering;
Lua does not decode cache files or perform per-frame rendering.
Other maps are unaffected. Noclip prevents falling through the terrain while
collision is not implemented.

## Validation

```bash
cargo fmt --manifest-path converter/Cargo.toml --all -- --check
cargo test --locked --manifest-path converter/Cargo.toml
cargo clippy --locked --manifest-path converter/Cargo.toml --all-targets -- -D warnings
bash -n convert.sh
./convert.sh regions 50 50 --cache /path/to/cache --preview-only
python tests/validate_region.py output/2009scape/regions/50_50
python tests/test_script.py
python tests/test_preview.py  # optional liblua 5.3/5.4 for the lifecycle harness
python tests/test_hlsl.py     # optional native Microsoft HLSL compiler harness
```

For GLua linting, install [GLuaFixer](https://github.com/FPtje/GLuaFixer) and
run `glualint lint addons tests` from the repository root. The linter reads
`glualint.json` automatically.

The GMod LuaLS workflow uses Garry's Mod API definitions and checks all source
files with the warning groups in `.luarc.json`. LuaLS's table brace spacing
matches GLuaFixer. LuaLS annotates warnings and fails on errors.

Run `ruff check .` to lint Python and `ruff format .` to apply formatting.
Windows CI runs linting and `ruff format --check .` using `ruff.toml`.

GitHub Actions runs these checks on **Windows x64 only** using the pinned
nightly toolchain, with a matrix for debug (`dev`) and optimized (`release`)
profiles. Both profiles run Clippy, tests and builds. Formatting differences and
build/test/lint errors fail the workflow, including every unsuppressed compiler
and Clippy warning. Only the release build uploads the CLI as the
**garryscape-convert-windows-x64.7z** artifact. `convert.sh` also builds and runs
the optimized release converter.
No RuneScape cache or compiled game assets are needed or uploaded by CI.

`converter/Cargo.toml` enables all 63 opt-in compiler lints in the pinned nightly
toolchain (Rust 1.102.0-nightly), plus every public Clippy group: `all`,
`pedantic`, `nursery`, `restriction`, and `cargo`. Default compiler errors and
Clippy correctness errors retain their severity. Clippy checks run with
`cargo clippy`; ordinary builds run the compiler checks.

Clippy passes with `-- -D warnings`, and CI enforces that gate. Conflicting
style pairs select one convention, documented in `Cargo.toml`: for example,
`unreachable_pub` stays enabled while `redundant_pub_crate` is disabled. Global
exceptions are limited to conflicting rules and checks unsuitable for this CLI
(such as requiring `alloc` imports in a std binary or forbidding single-use
helpers). Native arithmetic, protocol endianness, validated table indexing,
CLI output and exact equality use local `#[allow(..., reason = "...")]`
attributes on the relevant items. Tests scope their fixture/unwrap exceptions
to test modules. The crate also documents exceptions for the nightly lint
feature gates.
No blanket `allow(warnings)` or Clippy group suppression is used.

Rust has no universal `-Wall` group, so the explicit opt-in list needs updating
when new compiler lints become available.
Nightly-only lint feature gates are enabled explicitly in the converter crate;
no `RUSTC_BOOTSTRAP` override is used. Run Cargo commands from the repository
root or its `converter` directory to select the pinned toolchain automatically.

For visual inspection, use the Blender loader above; Blender is not required to
convert or compile. Linux can export the inputs using `--preview-only`; actual
Windows StudioMDL and in-game rendering must be checked on Windows. Automated
script tests use a fake compiler to verify orchestration, not to claim a real
MDL compilation. The Lua harness checks map isolation, terrain spawning and
cleanup, missing-model diagnostics, and deferred player placement with noclip.

The Lua harness checks API wiring, not HLSL register allocation. To run the
native compiler regression test, build `tests/hlsl_compile.cpp` using the Windows
SDK (or Winelib), and set `GARRYSCAPE_HLSL_COMPILER` to that executable and
`GARRYSCAPE_EGSM_INCLUDES` to EGSM's `include_files` directory. The helper loads
Microsoft's `d3dcompiler_47.dll`; `GARRYSCAPE_D3DCOMPILER_DLL` can specify another
location. It compiles all ten vertex and ten pixel variants using EGSM's default
flags and reports temporary-register usage. Compiler DLLs, upstream headers,
and compiled test output are not included in this repository. These tests verify
compilation; actual GPU rendering still requires an in-game check.

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
Public License, version 3 or later ([AGPL-3.0-or-later](LICENSE)). Adapted rs-map-viewer code
retains its BSD-2-Clause notice in `third_party/rs-map-viewer-LICENSE`.
Game/cache assets are not included or relicensed by this repository.
