#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Compile actual EGSM HLSL variants with Microsoft's Shader Model 3 compiler.

Build hlsl_compile.cpp with the Windows SDK, or Winelib, then set
GARRYSCAPE_HLSL_COMPILER and GARRYSCAPE_EGSM_INCLUDES. No compiler DLLs, shader
includes or compiled output are bundled. This checks compilation, not GPU drawing.
"""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class HlslCompilation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.compiler = os.environ.get('GARRYSCAPE_HLSL_COMPILER')
        cls.includes = os.environ.get('GARRYSCAPE_EGSM_INCLUDES')
        if not cls.compiler or not cls.includes:
            raise unittest.SkipTest('Set GARRYSCAPE_HLSL_COMPILER and GARRYSCAPE_EGSM_INCLUDES for native HLSL tests')
        text = (ROOT / 'addons/garryscape/lua/garryscape/atlas_shader.lua').read_text()
        cls.stages = dict(re.findall(r'shaderlib\.Compile(Vertex|Pixel)Shader\([^\n]+\[==\[(.*?)\]==\]\)', text, re.S))

    def compile_stage(self, stage, profile):
        with tempfile.TemporaryDirectory(prefix='garryscape hlsl ') as folder:
            source = Path(folder) / (stage + '.hlsl')
            source.write_text(self.stages[stage])
            result = subprocess.run([self.compiler, str(source), profile, self.includes, 'all'],
                                    capture_output=True, text=True, timeout=180)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            variants = re.findall(r'PASS ' + profile + r' lights=(\d) mode=(\d)', result.stdout)
            self.assertEqual(set(variants), {(str(light), str(mode)) for light in range(5) for mode in range(2)}, result.stdout)
            registers = list(map(int, re.findall(r'highest_temp=r(\d+)', result.stdout)))
            self.assertEqual(len(registers), 10, result.stdout)
            self.assertLessEqual(max(registers), 31, result.stdout)
            print(result.stdout, end='')

    def test_vertex_lighting_and_skinning_variants(self):
        self.compile_stage('Vertex', 'vs_3_0')

    def test_pixel_lighting_and_flashlight_variants(self):
        self.compile_stage('Pixel', 'ps_3_0')


if __name__ == '__main__':
    unittest.main()
