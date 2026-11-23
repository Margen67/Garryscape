# SPDX-License-Identifier: AGPL-3.0-or-later
"""Run with Python to open Blender, or run directly inside Blender.

The mesh/UVs stay untouched. Nodes reproduce independent atlas texture wrapping.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Literal, TypeAlias, cast

JsonObject: TypeAlias = dict[str, Any]
SocketValue: TypeAlias = "bpy.types.NodeSocket | float"
MathOperation: TypeAlias = Literal[
    "ADD",
    "DIVIDE",
    "FLOOR",
    "FLOORED_MODULO",
    "FRACT",
    "GREATER_THAN",
    "MAXIMUM",
    "MINIMUM",
    "MULTIPLY",
    "SUBTRACT",
]

MIN_MASK_DETERMINANT = 0.5
TERRAIN_UV_LAYER = 1
BLEND_LAYER_COUNT = 5

try:
    import bpy  # pyright: ignore[reportMissingModuleSource]  # Blender supplies bpy at runtime.
except ModuleNotFoundError as error:
    if error.name != "bpy":
        raise

    import os
    import shutil
    import subprocess  # ruff: ignore[suspicious-subprocess-import]

    blender = shutil.which("blender")
    steam_root = os.environ.get("PROGRAMFILES(X86)")
    if blender is None and steam_root:
        steam_blender = Path(steam_root) / "Steam/steamapps/common/Blender/blender.exe"
        if steam_blender.is_file():
            blender = str(steam_blender)
    if blender is None:
        msg = "Blender was not found. Add it to PATH or run this script inside Blender."
        raise SystemExit(msg) from None

    script = Path(__file__).resolve()
    result = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true]
        [blender, "--python-exit-code", "1", "--python", str(script)],
        check=False,
    )
    raise SystemExit(result.returncode) from None

editor = bpy.context.space_data
if isinstance(editor, bpy.types.SpaceTextEditor) and editor.text and editor.text.filepath:
    folder = Path(bpy.path.abspath(editor.text.filepath)).resolve().parent
else:
    folder = Path(__file__).resolve().parent
paths = list(folder.glob("*.gltf"))
if len(paths) != 1:
    msg = "Expected one glTF in the preview directory"
    raise RuntimeError(msg)
path = paths[0]
doc = json.loads(path.read_text())
startup_types = {"Camera": "CAMERA", "Cube": "MESH", "Light": "LIGHT"}
scene = cast("bpy.types.Scene | None", bpy.context.scene)
startup_objects = tuple(scene.objects) if scene is not None else ()
clear_startup_scene = (
    not bpy.data.filepath
    and {obj.name: obj.type for obj in startup_objects} == startup_types
    and all(
        obj.data is not None and obj.data.name == obj.name and len(obj.users_scene) == 1
        for obj in startup_objects
    )
)
before = set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=str(path))
objects = [o for o in bpy.data.objects if o not in before]
texture = doc["textures"][0]
source = texture.get("source")
if source is None:
    source = texture["extensions"]["EXT_texture_webp"]["source"]
atlas = bpy.data.images.load(str(folder / doc["images"][source]["uri"]), check_existing=True)


def _input_socket(node: bpy.types.Node, key: int | str) -> bpy.types.NodeSocket:
    return node.inputs[key]


def _output_socket(node: bpy.types.Node, key: int | str) -> bpy.types.NodeSocket:
    return node.outputs[key]


def native_color(  # ruff: ignore[too-many-locals, too-many-statements]
    tree: bpy.types.NodeTree,
    entry: JsonObject,
    source_uv: bpy.types.NodeSocket | None = None,
) -> tuple[bpy.types.NodeSocket, SocketValue]:
    """Build native atlas sampling nodes for a floor material.

    Returns:
        The color socket and either the alpha socket or a constant alpha.

    """
    data = entry["extras"]["garryscapeAtlas"]
    nodes = tree.nodes
    links = tree.links
    tint = entry["pbrMetallicRoughness"]["baseColorFactor"]
    if data["nativeTexture"] < 0:
        color = nodes.new("ShaderNodeRGB")
        cast("bpy.types.NodeSocketColor", color.outputs[0]).default_value = tint
        return color.outputs[0], tint[3]
    x, y, w, h = data["rect"]
    width = data["atlasWidth"]
    height = data["atlasHeight"]
    if source_uv is None:
        uv = nodes.new("ShaderNodeTexCoord")
        source_uv = _output_socket(uv, "UV")
        flipped = True
    else:
        flipped = False
    separate = nodes.new("ShaderNodeSeparateXYZ")
    links.new(source_uv, separate.inputs[0])

    def math(
        operation: MathOperation, a: SocketValue, b: SocketValue | None = None
    ) -> bpy.types.NodeSocket:
        node = cast("bpy.types.ShaderNodeMath", nodes.new("ShaderNodeMath"))
        node.operation = operation
        for i, v in enumerate([a] if b is None else [a, b]):
            if isinstance(v, (int, float)):
                cast("bpy.types.NodeSocketFloat", _input_socket(node, i)).default_value = v
            else:
                links.new(v, _input_socket(node, i))
        return _output_socket(node, 0)

    # Blender's glTF importer flips V into Blender UV convention. Recover the
    # original glTF UV before wrapping each native image independently.
    u = math("SUBTRACT", math("MULTIPLY", separate.outputs["X"], w), 0.5)
    v = math(
        "SUBTRACT",
        math(
            "MULTIPLY",
            math("SUBTRACT", 1, separate.outputs["Y"]) if flipped else separate.outputs["Y"],
            h,
        ),
        0.5,
    )
    fx = math("FRACT", u)
    fy = math("FRACT", v)
    ix = math("FLOOR", u)
    iy = math("FLOOR", v)
    samples = []
    weights = []
    for dy in range(2):
        for dx in range(2):
            px = math("FLOORED_MODULO", math("ADD", ix, dx), w)
            py = math("FLOORED_MODULO", math("ADD", iy, dy), h)
            au = math("DIVIDE", math("ADD", px, x + 0.5), width)
            av = math("SUBTRACT", 1, math("DIVIDE", math("ADD", py, y + 0.5), height))
            combine = nodes.new("ShaderNodeCombineXYZ")
            links.new(au, combine.inputs[0])
            links.new(av, combine.inputs[1])
            image = cast("bpy.types.ShaderNodeTexImage", nodes.new("ShaderNodeTexImage"))
            image.image = atlas
            image.interpolation = "Closest"
            image.extension = "EXTEND"
            links.new(combine.outputs[0], _input_socket(image, "Vector"))
            weight = math(
                "MULTIPLY",
                fx if dx else math("SUBTRACT", 1, fx),
                fy if dy else math("SUBTRACT", 1, fy),
            )
            multiply = cast("bpy.types.ShaderNodeVectorMath", nodes.new("ShaderNodeVectorMath"))
            multiply.operation = "SCALE"
            links.new(_output_socket(image, "Color"), _input_socket(multiply, 0))
            links.new(weight, _input_socket(multiply, "Scale"))
            samples.append(_output_socket(multiply, "Vector"))
            weights.append(math("MULTIPLY", _output_socket(image, "Alpha"), weight))

    def add_vectors(a: bpy.types.NodeSocket, b: bpy.types.NodeSocket) -> bpy.types.NodeSocket:
        node = cast("bpy.types.ShaderNodeVectorMath", nodes.new("ShaderNodeVectorMath"))
        node.operation = "ADD"
        links.new(a, _input_socket(node, 0))
        links.new(b, _input_socket(node, 1))
        return _output_socket(node, "Vector")

    result = add_vectors(add_vectors(samples[0], samples[1]), add_vectors(samples[2], samples[3]))
    multiply = cast("bpy.types.ShaderNodeVectorMath", nodes.new("ShaderNodeVectorMath"))
    multiply.operation = "MULTIPLY"
    links.new(result, _input_socket(multiply, 0))
    cast("bpy.types.NodeSocketVector", _input_socket(multiply, 1)).default_value = tint[:3]
    alpha = math("ADD", math("ADD", weights[0], weights[1]), math("ADD", weights[2], weights[3]))
    return _output_socket(multiply, "Vector"), math("MULTIPLY", alpha, tint[3])


def apply_material(material: bpy.types.Material, entry: JsonObject) -> None:
    """Replace an imported material with native atlas sampling nodes.

    Raises:
        RuntimeError: If Blender cannot provide a material node tree.

    """
    if material.node_tree is None:
        # Older Blender versions can create materials without a node tree.
        material.use_nodes = True
    tree = material.node_tree
    if tree is None:
        msg = "Blender did not create a material node tree"
        raise RuntimeError(msg)
    tree.nodes.clear()
    output = tree.nodes.new("ShaderNodeOutputMaterial")
    bsdf = tree.nodes.new("ShaderNodeBsdfPrincipled")
    cast("bpy.types.NodeSocketFloat", bsdf.inputs["Roughness"]).default_value = 1
    tree.links.new(bsdf.outputs["BSDF"], output.inputs["Surface"])
    color, alpha = native_color(tree, entry)
    tree.links.new(color, bsdf.inputs["Base Color"])
    if entry.get("alphaMode") == "BLEND" and not isinstance(alpha, (int, float)):
        tree.links.new(alpha, bsdf.inputs["Alpha"])


# The import may append suffixes to existing material names; pair by
# imported slots.
meshes = [obj.data for obj in objects if isinstance(obj.data, bpy.types.Mesh)]
materials = {material for mesh in meshes for material in mesh.materials if material is not None}
for material in materials:
    binding = (material.get("garryscapeAtlas") or {}).get("binding")
    if binding is None:
        raise RuntimeError("Imported atlas metadata missing: " + material.name)
    apply_material(material, doc["materials"][int(binding)])
# Reuse analytic mask and native-image node groups across all terrain materials.
# No terrain geometry or source texture is rasterized for the preview.
blend_path = folder / "ground_blending.json"
if blend_path.exists():
    import math as pymath

    controls = json.loads(blend_path.read_text())
    blending = controls["blending"]
    mapping = controls["mapping"]
    mask_groups = {}
    image_groups = {}
    blend_materials = {}

    def assign(tree: bpy.types.NodeTree, value: SocketValue, socket: bpy.types.NodeSocket) -> None:
        """Set a socket from a constant or an existing node output."""
        if isinstance(value, (int, float)):
            cast("bpy.types.NodeSocketFloat", socket).default_value = value
        else:
            tree.links.new(value, socket)

    def calc(
        tree: bpy.types.NodeTree,
        op: MathOperation,
        a: SocketValue,
        b: SocketValue | None = None,
    ) -> bpy.types.NodeSocket:
        """Create a math node and return its first output.

        Returns:
            The math node output socket.

        """
        node = cast("bpy.types.ShaderNodeMath", tree.nodes.new("ShaderNodeMath"))
        node.operation = op
        assign(tree, a, _input_socket(node, 0))
        if b is not None:
            assign(tree, b, _input_socket(node, 1))
        return _output_socket(node, 0)

    def group(
        name: str,
        output_type: str,
    ) -> tuple[bpy.types.ShaderNodeTree, bpy.types.Node, bpy.types.Node]:
        """Create a shader node group with local coordinates as input.

        Returns:
            The shader group, its coordinate splitter, and its output node.

        Raises:
            RuntimeError: If Blender cannot provide the shader group interface.

        """
        # The generated RNA stub omits ShaderNodeTree from the dynamic enum.
        nodes_group = bpy.data.node_groups.new(
            name,
            "ShaderNodeTree",  # pyright: ignore[reportArgumentType]
        )
        g = cast("bpy.types.ShaderNodeTree", nodes_group)
        interface = g.interface
        if interface is None:
            msg = "Blender did not create a shader group interface"
            raise RuntimeError(msg)
        interface.new_socket(name="XY", in_out="INPUT", socket_type="NodeSocketVector")
        interface.new_socket(name="Result", in_out="OUTPUT", socket_type=output_type)
        inp = g.nodes.new("NodeGroupInput")
        out = g.nodes.new("NodeGroupOutput")
        xy = g.nodes.new("ShaderNodeSeparateXYZ")
        g.links.new(inp.outputs[0], xy.inputs[0])
        return g, xy, out

    def mask_group(triangles: list[list[list[int]]]) -> bpy.types.ShaderNodeTree:
        """Build the interpolation mask for a set of blend triangles.

        Returns:
            The shader group computing the blend opacity.

        """
        key = json.dumps(triangles, separators=(",", ":"))
        if key in mask_groups:
            return mask_groups[key]
        g, xy, out = group("RS ground mask", "NodeSocketFloat")
        opacity = 0

        def linear(a: float, b: float, c: float) -> bpy.types.NodeSocket:
            return calc(
                g,
                "ADD",
                calc(
                    g,
                    "ADD",
                    calc(g, "MULTIPLY", xy.outputs["X"], a),
                    calc(g, "MULTIPLY", xy.outputs["Y"], b),
                ),
                c,
            )

        for a, b, c in triangles:
            ab = [b[i] - a[i] for i in range(2)]
            ac = [c[i] - a[i] for i in range(2)]
            det = ab[0] * ac[1] - ab[1] * ac[0]
            if abs(det) < MIN_MASK_DETERMINANT:
                continue
            u = linear(ac[1] / det, -ac[0] / det, (-a[0] * ac[1] + a[1] * ac[0]) / det)
            v = linear(-ab[1] / det, ab[0] / det, (ab[1] * a[0] - ab[0] * a[1]) / det)
            w = calc(g, "SUBTRACT", 1, calc(g, "ADD", u, v))
            inside = calc(
                g, "GREATER_THAN", calc(g, "MINIMUM", calc(g, "MINIMUM", u, v), w), -0.00001
            )
            alpha = calc(
                g,
                "ADD",
                a[2] / 255,
                calc(
                    g,
                    "ADD",
                    calc(g, "MULTIPLY", u, (b[2] - a[2]) / 255),
                    calc(g, "MULTIPLY", v, (c[2] - a[2]) / 255),
                ),
            )
            opacity = calc(g, "MAXIMUM", opacity, calc(g, "MULTIPLY", inside, alpha))
        assign(g, opacity, out.inputs[0])
        mask_groups[key] = g
        return g

    def image_group(floor_id: int) -> bpy.types.ShaderNodeTree:
        """Reuse or create the atlas sampling group for a floor.

        Returns:
            The shader group that samples the floor color.

        """
        if floor_id in image_groups:
            return image_groups[floor_id]
        f = controls["floors"][str(floor_id)]
        g, xy, out = group("RS floor " + str(floor_id), "NodeSocketColor")
        uv = g.nodes.new("ShaderNodeCombineXYZ")
        for i in range(2):
            value = calc(
                g,
                "ADD",
                calc(g, "DIVIDE", xy.outputs[i], f["scale"]),
                (mapping[i + 2] / f["scale"]) % 1,
            )
            g.links.new(value, _input_socket(uv, i))
        entry = {
            "extras": {
                "garryscapeAtlas": {
                    "rect": f["rect"],
                    "atlasWidth": atlas.size[0],
                    "atlasHeight": atlas.size[1],
                    "nativeTexture": f["texture"],
                }
            },
            "pbrMetallicRoughness": {"baseColorFactor": f["tint"]},
        }
        color, _ = native_color(g, entry, _output_socket(uv, 0))
        g.links.new(color, out.inputs[0])
        image_groups[floor_id] = g
        return g

    def blended_material(  # ruff: ignore[too-many-locals]
        base: bpy.types.Material,
        index: int,
        uv_name: str,
    ) -> bpy.types.Material:
        """Reuse or create a material for the requested blend plan.

        Returns:
            The Blender material for the specified base and blend plan.

        Raises:
            RuntimeError: If the copied material lacks a node tree.

        """
        key = (base.name, index, uv_name)
        if key in blend_materials:
            return blend_materials[key]
        material = base.copy()
        material.name = base.name + " blend " + str(index)
        tree = material.node_tree
        if tree is None:
            msg = "Copied atlas material is missing its node tree"
            raise RuntimeError(msg)
        nodes = tree.nodes
        links = tree.links
        bsdf = next(n for n in nodes if n.type == "BSDF_PRINCIPLED")
        color = bsdf.inputs["Base Color"].links[0].from_socket
        uv = cast("bpy.types.ShaderNodeUVMap", nodes.new("ShaderNodeUVMap"))
        uv.uv_map = uv_name
        xy = nodes.new("ShaderNodeSeparateXYZ")
        links.new(_output_socket(uv, 0), xy.inputs[0])
        y = calc(tree, "SUBTRACT", 1, xy.outputs["Y"])
        layer = calc(tree, "FLOOR", calc(tree, "DIVIDE", y, 128))
        values = [
            calc(tree, "MULTIPLY", xy.outputs["X"], 128),
            calc(
                tree, "MULTIPLY", calc(tree, "SUBTRACT", y, calc(tree, "MULTIPLY", layer, 128)), 128
            ),
        ]
        terrain = nodes.new("ShaderNodeCombineXYZ")
        local = nodes.new("ShaderNodeCombineXYZ")
        for i, value in enumerate(values):
            links.new(value, terrain.inputs[i])
            tile = calc(
                tree,
                "MINIMUM",
                63,
                calc(tree, "MAXIMUM", 0, calc(tree, "FLOOR", calc(tree, "DIVIDE", value, 128))),
            )
            links.new(
                calc(tree, "SUBTRACT", value, calc(tree, "MULTIPLY", tile, 128)), local.inputs[i]
            )
        for layer_plan in blending["plans"][index]:
            mask = cast("bpy.types.ShaderNodeGroup", nodes.new("ShaderNodeGroup"))
            mask.node_tree = mask_group(layer_plan["triangles"])
            links.new(local.outputs[0], mask.inputs[0])
            image = cast("bpy.types.ShaderNodeGroup", nodes.new("ShaderNodeGroup"))
            image.node_tree = image_group(layer_plan["floor"])
            links.new(terrain.outputs[0], _input_socket(image, 0))
            mix = cast("bpy.types.ShaderNodeMixRGB", nodes.new("ShaderNodeMixRGB"))
            mix.blend_type = "MIX"
            links.new(mask.outputs[0], _input_socket(mix, 0))
            links.new(color, _input_socket(mix, 1))
            links.new(_output_socket(image, 0), _input_socket(mix, 2))
            color = _output_socket(mix, 0)
        links.new(color, bsdf.inputs["Base Color"])
        blend_materials[key] = material
        return material

    for mesh in meshes:
        if len(mesh.uv_layers) <= TERRAIN_UV_LAYER:
            continue
        uv_layer = mesh.uv_layers[TERRAIN_UV_LAYER]
        slots = {}
        for poly in mesh.polygons:
            base = mesh.materials[poly.material_index]
            if base is None:
                continue
            if (
                doc["materials"][int((base.get("garryscapeAtlas") or {}).get("binding", 0))].get(
                    "alphaMode"
                )
                == "BLEND"
            ):
                continue
            coords = [uv_layer.data[i].uv for i in poly.loop_indices]
            x = sum(v.x for v in coords) / len(coords)
            y = sum(1 - v.y for v in coords) / len(coords)
            layer = pymath.floor(y / 128)
            y -= layer * 128
            tx = max(0, min(63, pymath.floor(x)))
            ty = max(0, min(63, pymath.floor(y)))
            index = (
                blending["grid"][(layer * 64 + tx) * 64 + ty]
                if 0 <= layer < BLEND_LAYER_COUNT
                else 0
            )
            if not index:
                continue
            material = blended_material(base, index, uv_layer.name)
            if material.name not in slots:
                slots[material.name] = len(mesh.materials)
                mesh.materials.append(material)
            poly.material_index = slots[material.name]

    print("Garryscape: analytic ground blending enabled in Blender")  # ruff: ignore[print]

if clear_startup_scene and any(obj.type == "MESH" for obj in objects):
    for obj in startup_objects:
        bpy.data.objects.remove(obj, do_unlink=True)

for obj in bpy.context.selected_objects:
    obj.select_set(state=False)
for obj in objects:
    obj.select_set(state=True)
view_layer = cast("bpy.types.ViewLayer | None", bpy.context.view_layer)
if objects and view_layer is not None:
    view_layer.objects.active = objects[0]
screen = cast("bpy.types.Screen | None", bpy.context.screen)
if not bpy.app.background and screen is not None:
    for area in screen.areas:
        space = area.spaces.active
        if isinstance(space, bpy.types.SpaceView3D):
            space.shading.type = "MATERIAL"
            region = next((r for r in area.regions if r.type == "WINDOW"), None)
            if region:
                with bpy.context.temp_override(area=area, region=region):
                    bpy.ops.view3d.view_selected(use_all_regions=False)

# This status is visible when Blender is launched from a terminal.
print(  # ruff: ignore[print]
    "Garryscape: imported original triangles and UVs with native-size atlas sampling",
)
