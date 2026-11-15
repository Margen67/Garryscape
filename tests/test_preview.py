#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Run the Lua preview harness with an installed liblua, using Python's stdlib."""
import ctypes
import ctypes.util
from pathlib import Path
import os
import unittest

class Preview(unittest.TestCase):
    def test_lifecycle(self):
        name = ctypes.util.find_library('lua5.4') or ctypes.util.find_library('lua5.3')
        if not name:
            self.skipTest('liblua 5.3/5.4 is not installed; GMod does not need it')
        lib = ctypes.CDLL(name)
        lib.luaL_newstate.restype = ctypes.c_void_p
        lib.luaL_openlibs.argtypes = [ctypes.c_void_p]
        lib.luaL_loadfilex.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
        lib.luaL_loadfilex.restype = ctypes.c_int
        lib.lua_pcallk.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ssize_t, ctypes.c_void_p]
        lib.lua_pcallk.restype = ctypes.c_int
        lib.lua_tolstring.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]
        lib.lua_tolstring.restype = ctypes.c_char_p
        lib.lua_close.argtypes = [ctypes.c_void_p]
        old_cwd = Path.cwd()
        state = lib.luaL_newstate()
        try:
            os.chdir(Path(__file__).resolve().parents[1])
            lib.luaL_openlibs(state)
            status = lib.luaL_loadfilex(state, b'tests/test_preview.lua', None)
            if status == 0:
                status = lib.lua_pcallk(state, 0, 0, 0, 0, None)
            self.assertEqual(status, 0, lib.lua_tolstring(state, -1, None))
        finally:
            lib.lua_close(state)
            os.chdir(old_cwd)

if __name__ == '__main__':
    unittest.main()
