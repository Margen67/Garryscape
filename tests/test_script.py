#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Test PowerShell orchestration with a fake compiler, not a real MDL compile."""

import os
import shutil
import subprocess  # ruff: ignore[suspicious-subprocess-import]
import tempfile
import unittest
from pathlib import Path
from typing import override

PWSH = shutil.which("pwsh")

REPO = Path(__file__).resolve().parents[1]
CLI = """#!/usr/bin/env python3
import json, pathlib, sys
args=sys.argv[1:]
out=pathlib.Path(args[args.index('--output')+1])
ns=args[args.index('--name')+1]
key=f'{args[1]}_{args[2]}'
b=out/ns/'regions'/key
s=b/'source/garryscape'/ns/'regions'/key;s.mkdir(parents=True,exist_ok=True)
(s/'model.qc').write_text(f'$modelname "garryscape/{ns}/regions/{key}.mdl"\\n')
(b/'game').mkdir(exist_ok=True)
preview=b/'preview';preview.mkdir(exist_ok=True)
(preview/f'{key}.webp').write_bytes(b'atlas')
(preview/f'{key}.gltf').write_text('{}')
m=b/'game/materials/models/garryscape'/ns/'regions';m.mkdir(parents=True,exist_ok=True)
(m/f'{key}_0.vmt').write_text('material');(m/f'{key}.vtf').write_text('texture')
(b/'manifest.json').write_text(json.dumps({'model':f'models/garryscape/{ns}/regions/{key}.mdl'}))
(b/'complete.json').write_text('fingerprint')
"""
COMPILER = """#!/usr/bin/env python3
import os,pathlib,re,sys
args=sys.argv[1:]
assert args[0]=='-game'
game=pathlib.Path(args[1])
qc=pathlib.Path(args[2])
assert qc.is_absolute() and qc.is_file()
with open(os.environ['COUNTER'],'a') as f:f.write('compile\\n')
mode=os.environ.get('FAIL_MODE','')
if mode=='exit':sys.exit(3)
if mode=='nothing':sys.exit(0)
if mode=='message':print('ERROR: bad input');sys.exit(0)
model=re.search(r'"([^" ]+\\.mdl)"',qc.read_text()).group(1)
p=game/'models'/model;p.parent.mkdir(parents=True,exist_ok=True)
for suffix in ['mdl','vvd','dx90.vtx']:p.with_suffix('.'+suffix).write_text('compiled')
"""


@unittest.skipIf(PWSH is None or os.name == "nt", "Requires pwsh and POSIX executable fixtures")
class Script(unittest.TestCase):
    """Check converter and compiler orchestration in a temporary install."""

    # unittest calls setUp before each test, which initializes these fixtures.
    tmp: tempfile.TemporaryDirectory[str]  # pyright: ignore[reportUninitializedInstanceVariable]
    path: Path  # pyright: ignore[reportUninitializedInstanceVariable]
    repo: Path  # pyright: ignore[reportUninitializedInstanceVariable]
    bin: Path  # pyright: ignore[reportUninitializedInstanceVariable]
    compiler: Path  # pyright: ignore[reportUninitializedInstanceVariable]
    cache: Path  # pyright: ignore[reportUninitializedInstanceVariable]
    addon: Path  # pyright: ignore[reportUninitializedInstanceVariable]
    counter: Path  # pyright: ignore[reportUninitializedInstanceVariable]
    env: dict[str, str]  # pyright: ignore[reportUninitializedInstanceVariable]

    @override
    def setUp(self) -> None:
        """Create an isolated checkout, cache, and compiler fixture."""
        self.tmp = tempfile.TemporaryDirectory(prefix="garryscape script ")
        self.path = Path(self.tmp.name)
        self.repo = self.path / "repo with spaces"
        self.repo.mkdir()
        _ = shutil.copy(REPO / "convert.ps1", self.repo / "convert.ps1")
        _ = shutil.copy(REPO / "LICENSE", self.repo / "LICENSE")
        _ = shutil.copy(REPO / "rust-toolchain.toml", self.repo / "rust-toolchain.toml")
        (self.repo / "addons/garryscape/lua/autorun/server").mkdir(parents=True)
        _ = shutil.copy(
            REPO / "addons/garryscape/lua/autorun/server/garryscape_preview.lua",
            self.repo / "addons/garryscape/lua/autorun/server",
        )
        (self.repo / "addons/garryscape/lua/autorun/client").mkdir(parents=True)
        (self.repo / "addons/garryscape/lua/garryscape").mkdir(parents=True)
        _ = shutil.copy(
            REPO / "addons/garryscape/lua/autorun/client/garryscape_utilities.lua",
            self.repo / "addons/garryscape/lua/autorun/client",
        )
        _ = shutil.copy(
            REPO / "addons/garryscape/lua/autorun/client/garryscape_shader.lua",
            self.repo / "addons/garryscape/lua/autorun/client",
        )
        _ = shutil.copy(
            REPO / "addons/garryscape/lua/garryscape/atlas_shader.lua",
            self.repo / "addons/garryscape/lua/garryscape",
        )
        (self.repo / "converter/target/release").mkdir(parents=True)
        self.exe(self.repo / "converter/target/release/garryscape-convert", CLI)
        self.bin = self.path / "bin"
        self.bin.mkdir()
        self.exe(
            self.bin / "cargo",
            """#!/bin/sh
test "$PWD" = "$GARRYSCAPE_TEST_CARGO_ROOT" || exit 41
test -f rust-toolchain.toml || exit 42
test "$RUSTFLAGS" = "-Ctarget-cpu=native" || exit 43
exit 0
""",
        )
        self.compiler = self.path / "fake studiomdl.exe"
        self.exe(self.compiler, COMPILER)
        self.cache = self.path / "cache"
        self.cache.mkdir()
        _ = (self.cache / "main_file_cache.dat2").write_bytes(b"cache")
        self.addon = self.path / "installed addon"
        self.counter = self.path / "counter"
        self.env = dict(
            os.environ,
            PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
            COUNTER=str(self.counter),
            GARRYSCAPE_TEST_CARGO_ROOT=str(self.repo),
        )

    @override
    def tearDown(self) -> None:
        """Remove the fixture directory."""
        self.tmp.cleanup()

    @staticmethod
    def exe(p: Path, text: str) -> None:
        """Write an executable test fixture."""
        _ = p.write_text(text, encoding="utf-8")
        p.chmod(0o755)

    def run_script(
        self,
        *extra: str,
        fail: str | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Run the conversion script with the current fixture.

        Returns:
            The exit status and captured output of the conversion script.

        """
        env = self.env.copy()
        if fail:
            env["FAIL_MODE"] = fail
        args = [
            PWSH or "pwsh",
            "-NoProfile",
            "-File",
            str(self.repo / "convert.ps1"),
            "regions",
            "50",
            "50",
            "-Cache",
            str(self.cache),
            "-StudioMdl",
            str(self.compiler),
            "-Output",
            "relative output",
            "-Addon",
            str(self.addon),
            *extra,
        ]
        return subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true]
            args, env=env, cwd=self.path, check=False, text=True, capture_output=True
        )

    def test_success_reuse_and_no_source_install(self) -> None:
        """Reuse a successful build and install only game files."""
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr + r.stdout)
        model = self.addon / "models/garryscape/2009scape/regions/50_50.mdl"
        stale = self.addon / "old file.txt"
        _ = stale.write_text("stale")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr + r.stdout)
        self.assertEqual(self.counter.read_text().count("compile"), 1)
        self.assertTrue(model.is_file())
        self.assertFalse(stale.exists())
        self.assertFalse((self.path / "relative output/build").exists())
        self.assertTrue(
            (self.path / "relative output/2009scape/regions/50_50/compiled.json").is_file()
        )
        self.assertFalse(
            any(
                p.suffix in {".qc", ".dmx", ".glb", ".gltf", ".bin", ".png", ".webp"}
                for p in self.addon.rglob("*")
            )
        )
        self.assertTrue((self.addon / "lua/autorun/server/garryscape_preview.lua").is_file())
        self.assertTrue((self.addon / "lua/autorun/client/garryscape_shader.lua").is_file())
        self.assertTrue((self.addon / "lua/autorun/client/garryscape_utilities.lua").is_file())
        self.assertTrue((self.addon / "lua/garryscape/atlas_shader.lua").is_file())

    def test_default_installs_under_garrysmod_directory(self) -> None:
        """Resolve the default addon destination beneath garrysmod."""
        game = self.path / "Program Files (x86)/Steam/steamapps/common/GarrysMod"
        self.env["TEST_FAKE_COMPILER"] = str(self.compiler)
        r = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true]
            [
                PWSH or "pwsh",
                "-NoProfile",
                "-Command",
                """New-PSDrive -Name C -PSProvider FileSystem -Root $env:TEST_DRIVE | Out-Null
& $env:TEST_LAUNCHER regions 50 50 -Cache $env:TEST_CACHE `
    -StudioMdl $env:TEST_FAKE_COMPILER -Output 'relative output'
exit $LASTEXITCODE""",
            ],
            env={
                **self.env,
                "TEST_DRIVE": str(self.path),
                "TEST_LAUNCHER": str(self.repo / "convert.ps1"),
                "TEST_CACHE": str(self.cache),
            },
            cwd=self.path,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(r.returncode, 0, r.stderr + r.stdout)
        addon = game / "garrysmod/addons/garryscape"
        self.assertTrue((addon / "models/garryscape/2009scape/regions/50_50.mdl").is_file())
        self.assertTrue(
            (addon / "materials/models/garryscape/2009scape/regions/50_50.vtf").is_file()
        )
        self.assertTrue((addon / "lua/autorun/server/garryscape_preview.lua").is_file())
        self.assertFalse((game / "addons/garryscape").exists())

    def test_rebuild_recompiles(self) -> None:
        """Invoke StudioMDL again when rebuilding."""
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(self.run_script("-Rebuild").returncode, 0)
        self.assertEqual(self.counter.read_text().count("compile"), 2)

    def test_stale_outputs_do_not_hide_empty_compile(self) -> None:
        """Reject a compiler run that leaves its expected outputs missing."""
        self.assertEqual(self.run_script().returncode, 0)
        r = self.run_script("-Rebuild", fail="nothing")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("did not produce", r.stderr)

    def test_failure_is_not_installed(self) -> None:
        """Keep failed compiler outputs out of the addon."""
        for mode in ["exit", "message"]:
            with self.subTest(mode=mode):
                r = self.run_script(fail=mode)
                self.assertNotEqual(r.returncode, 0, r.stdout)
                self.assertFalse(self.addon.exists())
                build = self.path / "relative output/2009scape/regions/50_50"
                log = build / "source/garryscape/2009scape/regions/50_50/compile.log"
                self.assertTrue(log.exists())

    def test_failure_preserves_previous_installation(self) -> None:
        """Keep the installed addon after a compiler failure."""
        self.assertEqual(self.run_script().returncode, 0)
        marker = self.addon / "keep.txt"
        _ = marker.write_text("previous installation")
        for mode in ["exit", "message", "nothing"]:
            r = self.run_script("-Rebuild", fail=mode)
            self.assertNotEqual(r.returncode, 0)
            self.assertEqual(marker.read_text(), "previous installation")

    def test_staging_failure_preserves_previous_installation(self) -> None:
        """Keep the installed addon after a staging failure."""
        self.assertEqual(self.run_script().returncode, 0)
        marker = self.addon / "keep.txt"
        _ = marker.write_text("previous installation")
        (self.repo / "LICENSE").unlink()
        r = self.run_script()
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(marker.read_text(), "previous installation")
        self.assertFalse(list(self.addon.parent.glob(".garryscape-install.*")))

    def test_source_destination_is_rejected(self) -> None:
        """Reject the checkout itself as an addon destination."""
        self.addon = self.repo
        r = self.run_script()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("would delete source", r.stderr)
        self.assertTrue((self.repo / "convert.ps1").is_file())

    def test_preview_does_not_compile_or_install(self) -> None:
        """Export the preview without running StudioMDL or installing the addon."""
        self.compiler.unlink()
        r = self.run_script("-PreviewOnly")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(self.counter.exists())
        self.assertFalse(self.addon.exists())
        self.assertTrue(
            (self.path / "relative output/2009scape/regions/50_50/preview/50_50.webp").is_file()
        )

    def test_region_required(self) -> None:
        """Reject conversion without both region coordinates."""
        r = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true]
            [
                PWSH or "pwsh",
                "-NoProfile",
                "-File",
                str(self.repo / "convert.ps1"),
                "regions",
                "50",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("Specify the region", r.stderr)

    def test_empty_namespace_uses_revision(self) -> None:
        """Use the cache revision when the namespace is empty."""
        r = self.run_script("-Name", "")
        self.assertEqual(r.returncode, 0, r.stderr + r.stdout)
        self.assertTrue((self.addon / "models/garryscape/530/regions/50_50.mdl").is_file())


if __name__ == "__main__":
    _ = unittest.main()
