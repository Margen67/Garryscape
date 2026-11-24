#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Git Bash / POSIX entry point. All RuneScape decoding runs in the Rust CLI.
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
usage() {
    cat <<'HELP'
Usage: ./convert.sh regions X Y [options]
  --cache PATH       Native 2009Scape cache (default: ${UserProfile}/2009scape/cache/runescape)
  --scale NUMBER     Model scale (default: 0.68)
  --name NAME        Namespace (default: 2009scape; empty string uses 530)
  --output PATH      Build/source/preview directory (default: ./output)
  --studiomdl PATH   GMod's bin/win64/studiomdl.exe
  --addon PATH       Compiled model/material destination
  --preview-only     Export glTF, DMX, QC and materials without compiling/installing
  --rebuild          Regenerate converted assets even if inputs have not changed
Only terrain is implemented. Region coordinates are required. No collision.
HELP
}
fail() { echo "ERROR: $*" >&2; exit 1; }
# MSYS2 must not reinterpret native Windows arguments passed to StudioMDL.
native_path() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s\n' "$1"; fi; }
shell_path() { if command -v cygpath >/dev/null 2>&1; then cygpath -u "$1"; else printf '%s\n' "$1"; fi; }
if [[ $# == 0 || ${1:-} == --help || ${1:-} == -h ]]; then usage; exit 0; fi
[[ ${1:-} == regions ]] || fail 'Only regions are implemented.'
[[ $# -ge 3 && $2 =~ ^[0-9]{1,3}$ && $3 =~ ^[0-9]{1,3}$ ]] || fail 'Specify the region: ./convert.sh regions 50 50'
RX=$((10#$2)); RY=$((10#$3)); shift 3
(( RX <= 255 && RY <= 255 )) || fail 'Region coordinates must be 0..255.'
PROFILE=${UserProfile:-${USERPROFILE:-}}
CACHE=${PROFILE:+$PROFILE/2009scape/cache/runescape}
SCALE=0.68
NAME=2009scape
OUTPUT="$ROOT/output"
STUDIOMDL='C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/bin/win64/studiomdl.exe'
ADDON='C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/garrysmod/addons/garryscape'
PREVIEW=0
REBUILD=()
while (( $# )); do
    case "$1" in
        --preview-only) PREVIEW=1; shift ;;
        --rebuild) REBUILD=(--rebuild); shift ;;
        --cache|--scale|--name|--output|--studiomdl|--addon)
            (( $# >= 2 )) || fail "$1 requires a value"
            case "$1" in
                --cache) CACHE=$2 ;; --scale) SCALE=$2 ;; --name) NAME=$2 ;;
                --output) OUTPUT=$2 ;; --studiomdl) STUDIOMDL=$2 ;; --addon) ADDON=$2 ;;
            esac
            shift 2 ;;
        *) fail "Unknown option: $1" ;;
    esac
done
[[ -n $NAME ]] || NAME=530
[[ $NAME =~ ^[a-z0-9_-]+$ ]] || fail 'Namespace must use lowercase letters, digits, hyphens or underscores.'
[[ -n $CACHE ]] || fail 'Set UserProfile or pass --cache PATH.'
CACHE=$(shell_path "$CACHE")
OUTPUT=$(shell_path "$OUTPUT")
mkdir -p -- "$OUTPUT"
OUTPUT="$(cd -- "$OUTPUT" && pwd)"
STUDIOMDL=$(shell_path "$STUDIOMDL")
ADDON=$(shell_path "$ADDON")
if [[ -f $STUDIOMDL ]]; then STUDIOMDL="$(cd -- "$(dirname -- "$STUDIOMDL")" && pwd)/$(basename -- "$STUDIOMDL")"; fi
[[ -f "$CACHE/main_file_cache.dat2" || -f "$CACHE/main_file_cache.dat2.dat" || -f "$CACHE/LIVE/main_file_cache.dat2" || -f "$CACHE/LIVE/main_file_cache.dat2.dat" ]] || fail "2009Scape cache not found in $CACHE. Supply your revision-530 cache with --cache; no cache is downloaded."
if (( ! PREVIEW )); then [[ -f $STUDIOMDL ]] || fail "StudioMDL not found: $STUDIOMDL (use --preview-only to export without it)"; fi
command -v cargo >/dev/null 2>&1 || fail 'Nightly Rust/Cargo is required: https://rustup.rs'
# Resolve the pinned nightly even when this script is called outside the repo.
(
    cd -- "$ROOT"
    if [[ ${CARGO_ENCODED_RUSTFLAGS+x} ]]; then
        # Cargo separates encoded arguments with ASCII unit separators.
        export CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS:+${CARGO_ENCODED_RUSTFLAGS}$'\x1f'}-Ctarget-cpu=native"
    else
        export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Ctarget-cpu=native"
    fi
    cargo build --locked --release --manifest-path "$ROOT/converter/Cargo.toml"
)
CLI="$ROOT/converter/target/release/garryscape-convert"
[[ ! -f $CLI.exe ]] || CLI=$CLI.exe
"$CLI" regions "$RX" "$RY" --cache "$(native_path "$CACHE")" --scale "$SCALE" --name "$NAME" --output "$(native_path "$OUTPUT")" "${REBUILD[@]}"
BUILD="$OUTPUT/$NAME/regions/${RX}_${RY}"
SOURCE="$BUILD/source/garryscape/$NAME/regions/${RX}_${RY}"
GAME="$BUILD/game"
if (( PREVIEW )); then echo "Preview ready: $BUILD/preview/${RX}_${RY}.gltf"; exit 0; fi
echo "Addon destination: $ADDON"
MODEL="$GAME/models/garryscape/$NAME/regions/${RX}_${RY}"
# Keep failed compiler inputs and logs outside the addon for diagnosis.
TOOL_HASH=$(cksum "$STUDIOMDL")
SAVED_TOOL_HASH=$(cat "$BUILD/compiled-tool.txt" 2>/dev/null || true)
if (( ${#REBUILD[@]} )) || [[ $TOOL_HASH != "$SAVED_TOOL_HASH" ]] || [[ ! -f $BUILD/compiled.json ]] || ! cmp -s "$BUILD/complete.json" "$BUILD/compiled.json" || [[ ! -s $MODEL.mdl || ! -s $MODEL.vvd || ! -s $MODEL.dx90.vtx ]]; then
    echo "Compiling $SOURCE/model.qc"
    # Old outputs must not make a compiler that produced nothing look successful.
    rm -f -- "$MODEL.mdl" "$MODEL.vvd" "$MODEL.dx90.vtx" "$MODEL.dx80.vtx" "$MODEL.sw.vtx"
    if ! (cd -- "$SOURCE" && MSYS2_ARG_CONV_EXCL='*' "$STUDIOMDL" -game "$(native_path "$GAME")" "$(native_path "$SOURCE/model.qc")") 2>&1 | tee "$SOURCE/compile.log"; then
        fail "Compilation failed; inputs and log retained at $SOURCE"
    fi
    for suffix in mdl vvd dx90.vtx; do [[ -s $MODEL.$suffix ]] || fail "StudioMDL did not produce $MODEL.$suffix; see $SOURCE/compile.log"; done
    if grep -qi 'ERROR:' "$SOURCE/compile.log"; then fail "StudioMDL reported errors; see $SOURCE/compile.log"; fi
    cp -- "$BUILD/complete.json" "$BUILD/compiled.json"
    printf '%s\n' "$TOOL_HASH" > "$BUILD/compiled-tool.txt"
else
    echo 'Using compiled Source model.'
fi
# Prepare the entire replacement before removing the installed addon. Compiler
# or staging failures leave the previous installation intact.
[[ -n $ADDON ]] || fail 'Addon destination must not be empty.'
case "$(basename -- "$ADDON")" in /|.|..) fail 'Unsafe addon destination.' ;; esac
[[ ! -L $ADDON ]] || fail 'Addon destination must not be a symbolic link.'
mkdir -p -- "$(dirname -- "$ADDON")"
ADDON="$(cd -- "$(dirname -- "$ADDON")" && pwd)/$(basename -- "$ADDON")"
case "$ADDON" in /|*/.|*/..) fail 'Unsafe addon destination.' ;; esac
for protected in "$ROOT" "$OUTPUT" "$(cd -- "$CACHE" && pwd)"; do
    case "$protected/" in "$ADDON/"*) fail "Addon destination would delete source, build or cache files: $ADDON" ;; esac
done
STAGED=$(mktemp -d "$(dirname -- "$ADDON")/.garryscape-install.XXXXXX")
trap 'rm -rf -- "$STAGED"' EXIT
copy_asset() { local src=$1 dst=$2; mkdir -p -- "$(dirname -- "$dst")"; cp -- "$src" "$dst"; }
for suffix in mdl vvd dx90.vtx dx80.vtx sw.vtx; do
    asset="$MODEL.$suffix"
    [[ ! -f $asset ]] || copy_asset "$asset" "$STAGED/models/garryscape/$NAME/regions/$(basename -- "$asset")"
done
for asset in "$GAME/materials/models/garryscape/$NAME/regions/${RX}_${RY}.vtf" "$GAME/materials/models/garryscape/$NAME/regions/${RX}_${RY}_"*.vmt; do
    copy_asset "$asset" "$STAGED/materials/models/garryscape/$NAME/regions/$(basename -- "$asset")"
done
copy_asset "$ROOT/LICENSE" "$STAGED/LICENSE"
copy_asset "$ROOT/addons/garryscape/lua/autorun/server/garryscape_preview.lua" "$STAGED/lua/autorun/server/garryscape_preview.lua"
copy_asset "$ROOT/addons/garryscape/lua/autorun/client/garryscape_utilities.lua" "$STAGED/lua/autorun/client/garryscape_utilities.lua"
copy_asset "$ROOT/addons/garryscape/lua/autorun/client/garryscape_shader.lua" "$STAGED/lua/autorun/client/garryscape_shader.lua"
copy_asset "$ROOT/addons/garryscape/lua/garryscape/atlas_shader.lua" "$STAGED/lua/garryscape/atlas_shader.lua"
copy_asset "$BUILD/manifest.json" "$STAGED/data/garryscape/terrain_preview.json"
# Deliberately replace the whole addon, including stale files from older builds.
rm -rf -- "$ADDON"
mv -- "$STAGED" "$ADDON"
trap - EXIT
echo "Installed compiled region and preview Lua in $ADDON"
echo 'Sources, glTF and compile.log remain in the output directory.'
echo 'The atlas materials require EGSM on the game client. See .github/README.md.'
