#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent structural checks on a real converted region (no game assets bundled)."""
import json
import math
from pathlib import Path
import re
import struct
import zlib
import sys


def parse_dmx(path):
    text = path.read_text()
    assert text.startswith('<!-- dmx encoding keyvalues2 1 format model 1 -->')
    tokens = re.findall(r'"(?:\\.|[^"\\])*"|[{}\[\],]', text)
    it = iter(tokens)
    elements = {}
    for kind in it:
        assert next(it) == '{'
        fields = {}
        while (key := next(it)) != '}':
            field_type, value = next(it), next(it)
            if value == '[':
                value = []
                while (token := next(it)) != ']':
                    if token != ',':
                        value.append(token.strip('"'))
            else:
                value = value.strip('"')
            fields[key.strip('"')] = (field_type.strip('"'), value)
        uid = fields['id'][1]
        assert uid not in elements, 'duplicate element id'
        elements[uid] = (kind.strip('"'), fields)
    for kind, fields in elements.values():
        for field_type, value in fields.values():
            if field_type == 'element':
                assert value in elements, ('missing reference', value)
            if field_type == 'element_array':
                assert len(value) % 2 == 0
                assert all(value[i] == 'element' and value[i+1] in elements for i in range(0, len(value), 2))
        if kind == 'DmeVertexData':
            count = len(fields['positions'][1])
            assert 0 < count <= 7200 and count % 3 == 0
            for name in ('normals', 'textureCoordinates', 'jointWeights', 'jointIndices'):
                assert len(fields[name][1]) == count
            for name in ('positionsIndices', 'normalsIndices', 'textureCoordinatesIndices'):
                assert list(map(int, fields[name][1])) == list(range(count))
            assert set(fields['jointIndices'][1]) == {'0'}
            assert set(fields['jointWeights'][1]) == {'1'}
        if kind == 'DmeFaceSet':
            indices = list(map(int, fields['faces'][1]))
            assert len(indices) % 4 == 0
            assert all(indices[i] == -1 for i in range(3, len(indices), 4))
    assert sum(kind == 'DmeMesh' for kind, _ in elements.values()) == 1
    return set(elements)


def validate(root):
    root = Path(root)
    manifest = json.loads((root / 'manifest.json').read_text())
    namespace, region = manifest['namespace'], manifest['region']
    assert manifest['collision'] is False and manifest['textureCount'] == 1
    assert all(v > 0 for v in manifest['trianglesPerFloor'])
    source = root / f'source/garryscape/{namespace}/regions/{region}'
    ids = set()
    for path in sorted(source.glob('reference_*.dmx')):
        group = parse_dmx(path)
        assert not ids.intersection(group), 'DMX bodies share UUIDs'
        ids.update(group)
    qc = (source / 'model.qc').read_text()
    assert '$collision' not in qc and '$concave' not in qc
    assert len(re.findall(r'^\$body ', qc, re.M)) == len(list(source.glob('reference_*.dmx')))
    materials = root / f'game/materials/models/garryscape/{namespace}/regions'
    textures = list(materials.glob('*.vtf'))
    assert len(textures) == 1
    with textures[0].open('rb') as stream:
        header = stream.read(80)
    assert header[:4] == b'VTF\0' and struct.unpack_from('<II', header, 4) == (7, 2)
    width, height = struct.unpack_from('<HH', header, 16)
    assert (width, height) == (manifest['atlasWidth'], manifest['atlasHeight'])
    assert width >= 2 and height >= 2 and width & (width-1) == height & (height-1) == 0
    assert header[56] == max(width, height).bit_length()
    assert textures[0].stat().st_size == 80 + sum(4 * max(width >> i, 1) * max(height >> i, 1) for i in range(header[56]))
    base = textures[0].read_bytes()[-width*height*4:]
    for image in manifest['atlasImages']:
        x,y,w,h = image['rect']
        raw = b''.join(base[((y+j)*width+x)*4:((y+j)*width+x+w)*4] for j in range(h))
        assert zlib.crc32(raw) == image['crc'], 'native atlas image pixels changed'
    assert struct.unpack_from('<I', header, 20)[0] & 0x200, 'control data needs NOLOD protection'
    blend = manifest['groundBlending']
    cx,cy,side,_ = blend['rect']
    def control(index):
        assert 0 <= index < side*side
        offset = ((cy+index//side)*width+cx+index%side)*4
        return tuple(base[offset:offset+4])
    def address(p): return p[0]+256*p[1]+65536*p[2]
    # EGSM uses sRGB reads for the albedo sampler. The shader restores encoded
    # control bytes before interpreting addresses, masks and descriptor fields.
    for byte in range(256):
        encoded=byte/255
        linear=encoded/12.92 if encoded<=0.04045 else ((encoded+0.055)/1.055)**2.4
        restored=linear*12.92 if linear<=0.0031308 else 1.055*linear**(1/2.4)-0.055
        assert round(restored*255)==byte
    checked = set()
    for cell,plan in enumerate(blend['grid']):
        pointer = address(control(cell))
        if plan == 0:
            assert pointer == 0
            continue
        if pointer in checked: continue
        checked.add(pointer)
        layers = blend['plans'][plan]
        assert control(pointer)[0] == len(layers)
        pointer += 1
        for layer in layers:
            header = control(pointer);pointer += 1
            assert header[3] == len(layer['triangles'])
            descriptor = address(header)
            assert control(descriptor+4)[0]+256*control(descriptor+4)[1] > 0
            for triangle in layer['triangles']:
                for vertex in triangle:
                    assert control(pointer)[:3] == tuple(vertex)
                    pointer += 1
    assert checked, 'no ground blending controls exported'
    for path in materials.glob('*.vmt'):
        vmt = path.read_text()
        assert '\n\n' not in vmt and vmt.endswith('}\n')
        assert 'VertexLitGeneric' in vmt and '$surfaceprop' not in vmt
        assert f'models/garryscape/{namespace}/regions/{region}"' in vmt
        assert f'"$atlasdimensions" "[{width} {height} 0 0]"' in vmt
    folder = root / 'preview'
    doc = json.loads((folder / f'{region}.gltf').read_text())
    assert doc['asset']['version'] == '2.0'
    assert len(doc['buffers']) == 1 and doc['buffers'][0]['uri'] == f'{region}.bin'
    buffer = (folder / doc['buffers'][0]['uri']).read_bytes()
    assert len(buffer) == doc['buffers'][0]['byteLength']
    assert len(doc['textures']) == len(doc['meshes']) == 1
    texture = doc['textures'][0]
    webp_extension = 'EXT_texture_webp'
    sources = set()
    if 'source' in texture:
        source = texture['source']
        sources.add(source)
        assert doc['images'][source]['uri'] == f'{region}.png'
        png = (folder / f'{region}.png').read_bytes()
        assert png[:8] == b'\x89PNG\r\n\x1a\n'
        assert struct.unpack_from('>II', png, 16) == (width, height)
    if webp_extension in texture.get('extensions', {}):
        source = texture['extensions'][webp_extension]['source']
        sources.add(source)
        assert doc['images'][source]['uri'] == f'{region}.webp'
        assert webp_extension in doc['extensionsUsed']
        assert (webp_extension in doc.get('extensionsRequired', [])) == ('source' not in texture)
        webp = (folder / f'{region}.webp').read_bytes()
        assert webp[:4] == b'RIFF' and webp[8:16] == b'WEBPVP8L'
        assert webp[20] == 0x2f
        dimensions, = struct.unpack_from('<I', webp, 21)
        assert ((dimensions & 0x3fff) + 1, (dimensions >> 14 & 0x3fff) + 1) == (width, height)
    assert sources and sources == set(range(len(doc['images'])))
    assert all(not material['doubleSided'] for material in doc['materials'])
    count = sum(doc['accessors'][p['indices']]['count'] // 3 for p in doc['meshes'][0]['primitives'])
    assert count == manifest['triangles']
    if region == '50_50':
        assert count == 11475 and manifest['trianglesPerFloor'] == [8847, 1100, 724, 62]
        assert manifest['underwaterTriangles'] == 742
        assert (width, height) == (1024, 512)
        assert manifest['padding'] == 0 and manifest['resizedImages'] == 0
        assert any(m['alphaMode'] == 'BLEND' for m in doc['materials']), 'missing water transparency'
    def array(index, components):
        acc = doc['accessors'][index]
        view = doc['bufferViews'][acc['bufferView']]
        offset = view.get('byteOffset', 0) + acc.get('byteOffset', 0)
        return [struct.unpack_from('<' + 'f'*components, buffer, offset+i*components*4) for i in range(acc['count'])]
    vertices = array(0, 3)
    if region == '50_50':
        # Known cache shoreline vertices: the old exporter lowered the bed
        # endpoint 5.44–6.8 Source units below the adjoining bank here.
        unit = json.loads((root / 'complete.json').read_text())['scale'] * 0.5 * 128 * 0.0254
        for x, y in [(33.5, 58), (33.5, 59), (34.5, 49), (34.5, 50), (34.5, 56.75)]:
            px, pz = (x - 32) * unit, -(y - 32) * unit
            heights = [v[1] for v in vertices if abs(v[0]-px) < 1e-5 and abs(v[2]-pz) < 1e-5]
            assert heights and all(abs(h) < 1e-5 for h in heights), f'shoreline gap at {x}/{y}: {heights}'
    assert all('TEXCOORD_1' in p['attributes'] for p in doc['meshes'][0]['primitives'])
    normals = array(1, 3)
    assert all(all(math.isfinite(v) for v in a) for a in vertices + normals)
    assert all(abs(sum(v*v for v in n) - 1) < 1e-5 for n in normals)
    for i in range(0, len(vertices), 3):
        a, b, c = vertices[i:i+3]
        ab = [b[j]-a[j] for j in range(3)]
        ac = [c[j]-a[j] for j in range(3)]
        cross = [ab[1]*ac[2]-ab[2]*ac[1], ab[2]*ac[0]-ab[0]*ac[2], ab[0]*ac[1]-ab[1]*ac[0]]
        assert cross[1] > 0, 'backface or degenerate terrain face'
    print(f'PASS: {count} triangles, four floors, one texture, valid DMX graph/mips/winding, no collision')

if __name__ == '__main__':
    validate(sys.argv[1])
