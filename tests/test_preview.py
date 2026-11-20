#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Run the Lua preview harness with an installed liblua, using Python's stdlib."""
import ctypes
import ctypes.util
import os
import unittest
from pathlib import Path


class Preview(unittest.TestCase):
    """Exercise addon Lua setup in an isolated Lua state."""

    def run_lua(self, filename: str) -> None:
        """Load and execute one Lua harness."""
        name = ctypes.util.find_library(
            "lua5.4") or ctypes.util.find_library("lua5.3")
        if not name:
            self.skipTest(
                "liblua 5.3/5.4 is not installed; GMod does not need it")
        lib = ctypes.CDLL(name)
        lib.luaL_newstate.restype = ctypes.c_void_p
        lib.luaL_openlibs.argtypes = [ctypes.c_void_p]
        lib.luaL_loadfilex.argtypes = [
            ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
        lib.luaL_loadfilex.restype = ctypes.c_int
        lib.lua_pcallk.argtypes = [
            ctypes.c_void_p,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_ssize_t,
            ctypes.c_void_p]
        lib.lua_pcallk.restype = ctypes.c_int
        lib.lua_tolstring.argtypes = [
            ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]
        lib.lua_tolstring.restype = ctypes.c_char_p
        lib.lua_close.argtypes = [ctypes.c_void_p]
        old_cwd = Path.cwd()
        state = lib.luaL_newstate()
        try:
            os.chdir(Path(__file__).resolve().parents[1])
            lib.luaL_openlibs(state)
            status = lib.luaL_loadfilex(state, filename.encode(), None)
            if status == 0:
                status = lib.lua_pcallk(state, 0, 0, 0, 0, None)
            self.assertEqual(status, 0, lib.lua_tolstring(state, -1, None))
        finally:
            lib.lua_close(state)
            os.chdir(old_cwd)

    def test_lifecycle(self) -> None:
        """Check terrain spawning, cleanup, and player placement."""
        self.run_lua("tests/test_preview.lua")

    def test_utilities_panel(self) -> None:
        """Check the ground blending toggle in Utilities."""
        self.run_lua("tests/test_utilities.lua")

    def test_native_shader_setup(self) -> None:
        """Check native atlas shader registration and parameters."""
        self.run_lua("tests/test_atlas_shader.lua")


if __name__ == "__main__":
    unittest.main()
