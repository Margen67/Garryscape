#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Independent structural checks on a real converted region (no game assets bundled)."""
import json
import math
from pathlib import Path
import re
import struct
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
    assert width == height == manifest['atlasSize']
    assert header[56] == width.bit_length()
    assert textures[0].stat().st_size == 80 + sum(4 * (width >> i)**2 for i in range(header[56]))
    for path in materials.glob('*.vmt'):
        vmt = path.read_text()
        assert '\n\n' not in vmt and vmt.endswith('}\n')
        assert 'VertexLitGeneric' in vmt and '$surfaceprop' not in vmt
        assert f'models/garryscape/{namespace}/regions/{region}"' in vmt
    folder = root / 'preview'
    doc = json.loads((folder / f'{region}.gltf').read_text())
    assert doc['asset']['version'] == '2.0'
    assert len(doc['buffers']) == 1 and doc['buffers'][0]['uri'] == f'{region}.bin'
    buffer = (folder / doc['buffers'][0]['uri']).read_bytes()
    assert len(buffer) == doc['buffers'][0]['byteLength']
    assert doc['images'][0]['uri'] == f'{region}.png'
    png = (folder / doc['images'][0]['uri']).read_bytes()
    assert png[:8] == b'\x89PNG\r\n\x1a\n'
    assert struct.unpack_from('>II', png, 16) == (width, height)
    assert len(doc['images']) == len(doc['textures']) == len(doc['meshes']) == 1
    assert all(not material['doubleSided'] for material in doc['materials'])
    count = sum(doc['accessors'][p['indices']]['count'] // 3 for p in doc['meshes'][0]['primitives'])
    assert count == manifest['triangles']
    if region == '50_50':
        assert count == 11475 and manifest['trianglesPerFloor'] == [8847, 1100, 724, 62]
        assert manifest['underwaterTriangles'] == 742
        assert manifest['padding'] == 0 and manifest['resizedImages'] == 0
        assert any(m['alphaMode'] == 'BLEND' for m in doc['materials']), 'missing water transparency'
    def array(index, components):
        acc = doc['accessors'][index]
        view = doc['bufferViews'][acc['bufferView']]
        offset = view.get('byteOffset', 0) + acc.get('byteOffset', 0)
        return [struct.unpack_from('<' + 'f'*components, buffer, offset+i*components*4) for i in range(acc['count'])]
    vertices = array(0, 3)
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
