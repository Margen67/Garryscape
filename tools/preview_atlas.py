# SPDX-License-Identifier: AGPL-3.0-only
"""Run with Blender: blender --python open_in_blender.py
The mesh/UVs stay untouched. Nodes reproduce independent atlas texture wrapping.
"""
from pathlib import Path
import json
import bpy

folder=Path(__file__).resolve().parent
paths=list(folder.glob('*.gltf'))
if len(paths)!=1:
    raise RuntimeError('Expected one glTF in the preview directory')
path=paths[0]
doc=json.loads(path.read_text())
before=set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=str(path))
objects=[o for o in bpy.data.objects if o not in before]
atlas=bpy.data.images.load(str(folder/doc['images'][0]['uri']),check_existing=True)

def apply_material(material,entry):
    data=entry['extras']['garryscapeAtlas']
    material.use_nodes=True
    nodes=material.node_tree.nodes;nodes.clear();links=material.node_tree.links
    output=nodes.new('ShaderNodeOutputMaterial')
    bsdf=nodes.new('ShaderNodeBsdfPrincipled');bsdf.inputs['Roughness'].default_value=1
    links.new(bsdf.outputs['BSDF'],output.inputs['Surface'])
    tint=entry['pbrMetallicRoughness']['baseColorFactor']
    if data['nativeTexture']<0:
        bsdf.inputs['Base Color'].default_value=tint
        return
    x,y,w,h=data['rect'];width=data['atlasWidth'];height=data['atlasHeight']
    uv=nodes.new('ShaderNodeTexCoord')
    separate=nodes.new('ShaderNodeSeparateXYZ');links.new(uv.outputs['UV'],separate.inputs[0])
    def math(operation,a,b=None):
        node=nodes.new('ShaderNodeMath');node.operation=operation
        for i,v in enumerate([a] if b is None else [a,b]):
            if isinstance(v,(int,float)):node.inputs[i].default_value=v
            else:links.new(v,node.inputs[i])
        return node.outputs[0]
    # Blender's glTF importer flips V into Blender UV convention. Recover the
    # original glTF UV before wrapping each native image independently.
    u=math('SUBTRACT',math('MULTIPLY',separate.outputs['X'],w),0.5)
    v=math('SUBTRACT',math('MULTIPLY',math('SUBTRACT',1,separate.outputs['Y']),h),0.5)
    fx=math('FRACT',u);fy=math('FRACT',v)
    ix=math('FLOOR',u);iy=math('FLOOR',v)
    samples=[];weights=[]
    for dy in range(2):
        for dx in range(2):
            px=math('FLOORED_MODULO',math('ADD',ix,dx),w)
            py=math('FLOORED_MODULO',math('ADD',iy,dy),h)
            au=math('DIVIDE',math('ADD',px,x+0.5),width)
            av=math('SUBTRACT',1,math('DIVIDE',math('ADD',py,y+0.5),height))
            combine=nodes.new('ShaderNodeCombineXYZ');links.new(au,combine.inputs[0]);links.new(av,combine.inputs[1])
            image=nodes.new('ShaderNodeTexImage');image.image=atlas;image.interpolation='Closest';image.extension='EXTEND'
            links.new(combine.outputs[0],image.inputs['Vector'])
            weight=math('MULTIPLY',fx if dx else math('SUBTRACT',1,fx),fy if dy else math('SUBTRACT',1,fy))
            multiply=nodes.new('ShaderNodeVectorMath');multiply.operation='SCALE'
            links.new(image.outputs['Color'],multiply.inputs[0]);links.new(weight,multiply.inputs['Scale'])
            samples.append(multiply.outputs['Vector'])
            weights.append(math('MULTIPLY',image.outputs['Alpha'],weight))
    def add_vectors(a,b):
        node=nodes.new('ShaderNodeVectorMath');node.operation='ADD'
        links.new(a,node.inputs[0]);links.new(b,node.inputs[1]);return node.outputs['Vector']
    result=add_vectors(add_vectors(samples[0],samples[1]),add_vectors(samples[2],samples[3]))
    multiply=nodes.new('ShaderNodeVectorMath');multiply.operation='MULTIPLY'
    links.new(result,multiply.inputs[0]);multiply.inputs[1].default_value=tint[:3]
    links.new(multiply.outputs['Vector'],bsdf.inputs['Base Color'])
    if entry.get('alphaMode')=='BLEND':
        alpha=math('ADD',math('ADD',weights[0],weights[1]),math('ADD',weights[2],weights[3]))
        links.new(math('MULTIPLY',alpha,tint[3]),bsdf.inputs['Alpha'])

# The import may append suffixes to existing material names; pair by imported slots.
materials={m for obj in objects if obj.type=='MESH' for m in obj.data.materials}
for material in materials:
    binding=material.get('garryscapeAtlas',{}).get('binding')
    if binding is None:raise RuntimeError('Imported atlas metadata missing: '+material.name)
    apply_material(material,doc['materials'][int(binding)])
for obj in bpy.context.selected_objects:obj.select_set(False)
for obj in objects:obj.select_set(True)
if objects:bpy.context.view_layer.objects.active=objects[0]
if not bpy.app.background and bpy.context.screen:
    for area in bpy.context.screen.areas:
        if area.type=='VIEW_3D':
            area.spaces.active.shading.type='MATERIAL'
            region=next((r for r in area.regions if r.type=='WINDOW'),None)
            if region:
                with bpy.context.temp_override(area=area,region=region):
                    bpy.ops.view3d.view_selected(use_all_regions=False)
print('Garryscape: imported original triangles and UVs with native-size atlas sampling')
