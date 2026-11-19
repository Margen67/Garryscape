# SPDX-License-Identifier: AGPL-3.0-only
"""Encode the preview atlas and configure its glTF texture sources."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory

WEBP = "EXT_texture_webp"


def encode_preview(build, region, mode):
    preview = build / "preview"
    gltf = preview / f"{region}.gltf"
    source = build / "preview-atlas.png"
    stamp = build / "preview-encoding.json"
    document = json.loads(gltf.read_text(encoding="utf-8"))
    texture = document["textures"][0]
    texture.pop("source", None)
    extensions = texture.setdefault("extensions", {})
    extensions.pop(WEBP, None)
    for key in ("extensionsUsed", "extensionsRequired"):
        if key in document:
            document[key] = [name for name in document[key] if name != WEBP]
            if not document[key]:
                del document[key]

    images = []
    if mode in ("both", "oxipng"):
        texture["source"] = len(images)
        images.append({"uri": f"{region}.png"})
    if mode in ("both", "webp"):
        extensions[WEBP] = {"source": len(images)}
        images.append({"uri": f"{region}.webp"})
        document.setdefault("extensionsUsed", []).append(WEBP)
        if mode == "webp":
            document.setdefault("extensionsRequired", []).append(WEBP)
    if not extensions:
        del texture["extensions"]
    document["images"] = images

    fingerprint = {
        "mode": mode,
        "atlas": hashlib.sha256(source.read_bytes()).hexdigest(),
        "atlas_mtime_ns": source.stat().st_mtime_ns,
        "encoder_script": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    }
    outputs = {image["uri"] for image in images}
    old_outputs = {f"{region}.png", f"{region}.webp"} - outputs
    if (stamp.is_file()
            and stamp.read_text(encoding="utf-8") == json.dumps(fingerprint)
            and json.loads(gltf.read_text(encoding="utf-8")) == document
            and all((preview / name).is_file() for name in outputs)
            and all(not (preview / name).exists() for name in old_outputs)):
        return

    with TemporaryDirectory(dir=preview) as directory:
        stage = Path(directory)
        if mode in ("both", "oxipng"):
            png = stage / f"{region}.png"
            shutil.copyfile(source, png)
            subprocess.run(["oxipng", "-o", "max", str(png)], check=True)
        if mode in ("both", "webp"):
            subprocess.run(
                ["cwebp", "-quiet", "-lossless", "-z", "9", "-exact",
                 str(source), "-o", str(stage / f"{region}.webp")],
                check=True,
            )
        staged_gltf = stage / gltf.name
        staged_gltf.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
        for name in outputs:
            (stage / name).replace(preview / name)
        staged_gltf.replace(gltf)
        for name in old_outputs:
            (preview / name).unlink(missing_ok=True)
        staged_stamp = stage / stamp.name
        staged_stamp.write_text(json.dumps(fingerprint), encoding="utf-8")
        staged_stamp.replace(stamp)


if __name__ == "__main__":
    build, region, mode = sys.argv[1:]
    try:
        encode_preview(Path(build), region, mode)
    except (OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"ERROR: Could not encode preview atlas: {error}")
