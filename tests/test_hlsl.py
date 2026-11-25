#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Compile actual EGSM HLSL variants with Microsoft's Shader Model 3 compiler.

Build tests/hlsl_compile.rs as a Windows executable, then set
GARRYSCAPE_HLSL_COMPILER and GARRYSCAPE_EGSM_INCLUDES. No compiler DLLs, shader
includes or compiled output are bundled. This checks compilation, not GPU drawing.
"""

import os
import re
import subprocess  # ruff: ignore[suspicious-subprocess-import] -- Runs the explicitly configured test compiler.
import tempfile
import unittest
from pathlib import Path
from typing import override

ROOT = Path(__file__).resolve().parents[1]


class HlslCompilation(unittest.TestCase):
    """Check compiled Shader Model 3 variants and register bounds."""

    @classmethod
    @override
    def setUpClass(cls) -> None:
        """Load the configured compiler and extract the embedded shaders.

        Raises:
            unittest.SkipTest: If the native compiler fixture is not configured.

        """
        compiler = os.environ.get("GARRYSCAPE_HLSL_COMPILER")
        includes = os.environ.get("GARRYSCAPE_EGSM_INCLUDES")
        if not compiler or not includes:
            msg = "Set GARRYSCAPE_HLSL_COMPILER and GARRYSCAPE_EGSM_INCLUDES for native HLSL tests"
            raise unittest.SkipTest(msg)
        cls.compiler = compiler
        cls.includes = includes
        text = (ROOT / "addons/garryscape/lua/garryscape/atlas_shader.lua").read_text()
        cls.stages = dict(
            re.findall(
                r"shaderlib\.Compile(Vertex|Pixel)Shader\([^\n]+\[==\[(.*?)\]==\]\)",
                text,
                re.DOTALL,
            )
        )

    def compile_stage(self, stage: str, profile: str) -> None:
        """Compile and validate all variants of the given shader stage."""
        with tempfile.TemporaryDirectory(prefix="garryscape hlsl ") as folder:
            source = Path(folder) / (stage + ".hlsl")
            _ = source.write_text(self.stages[stage])
            # The test operator explicitly selects the compiler.
            result = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true]
                [self.compiler, str(source), profile, self.includes, "all"],
                capture_output=True,
                text=True,
                timeout=180,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            variants = re.findall(r"PASS " + profile + r" lights=(\d) mode=(\d)", result.stdout)
            expected = {(str(light), str(mode)) for light in range(5) for mode in range(2)}
            self.assertEqual(set(variants), expected, result.stdout)
            registers = list(map(int, re.findall(r"highest_temp=r(\d+)", result.stdout)))
            self.assertEqual(len(registers), len(expected), result.stdout)
            self.assertLessEqual(max(registers), 31, result.stdout)

    def test_vertex_lighting_and_skinning_variants(self) -> None:
        """Compile the vertex shader with lighting and skinning variants."""
        self.compile_stage("Vertex", "vs_3_0")

    def test_pixel_lighting_and_flashlight_variants(self) -> None:
        """Compile the pixel shader with lighting and flashlight variants."""
        self.compile_stage("Pixel", "ps_3_0")


if __name__ == "__main__":
    _ = unittest.main()
