-- SPDX-License-Identifier: AGPL-3.0-only
-- Exercise teleport hooks without pretending to run the Source engine.
local script = "addons/garryscape/lua/autorun/server/garryscape_preview.lua"
local hooks, timers, map, config
function Vector(x,y,z) return {x=x,y=y,z=z} end
function Angle(p,y,r) return {p=p,y=y,r=r} end
function istable(value) return type(value) == "table" end
function IsValid(value) return type(value) == "table" and not value.removed end
hook = { Add = function(event, _, fn) hooks[event] = fn end }
timer = { Simple = function(delay, fn) assert(delay == 0); timers[#timers+1] = fn end }
game = { GetMap = function() return map end }
file = { Read = function(path, realm)
    assert(path == "data/garryscape/terrain_preview.json" and realm == "GAME")
    return "manifest"
end }
util = { JSONToTable = function(_) return config end }
MOVETYPE_NOCLIP = 8
-- Any attempt to spawn models should fail this harness.
ents = { Create = function() error("preview Lua must not spawn entities") end }
local function player()
    return {
        SetPos = function(self, value) self.pos = value end,
        SetEyeAngles = function(self, value) self.eyeAngles = value end,
        SetMoveType = function(self, value) self.moveType = value end
    }
end
local function reset(whichMap, manifest)
    hooks, timers, map, config = {}, {}, whichMap, manifest
    dofile(script)
end
local function flush()
    for _, fn in ipairs(timers) do fn() end
    timers = {}
end

reset("gm_construct", nil)
assert(next(hooks) == nil, "preview must not affect other maps")

reset("garryscape", {sourceBounds={{-100,-100,0},{100,100,370}}})
assert(hooks.InitPostEntity == nil and hooks.PostCleanupMap == nil)
assert(hooks.Think == nil and hooks.PostDrawOpaqueRenderables == nil)
local ply = player()
hooks.PlayerSpawn(ply)
assert(ply.pos == nil, "teleport must be deferred past PlayerSpawn")
flush()
assert(ply.pos.x == 0 and ply.pos.y == 0 and ply.pos.z == -10430)
assert(ply.moveType == MOVETYPE_NOCLIP)
assert(ply.eyeAngles.p == 89 and ply.eyeAngles.y == 0 and ply.eyeAngles.r == 0)

reset("garryscape", nil)
ply = player()
hooks.PlayerSpawn(ply)
flush()
assert(ply.pos.z == -10400, "missing manifest uses the above-terrain fallback")
local disconnected = player()
hooks.PlayerSpawn(disconnected)
disconnected.removed = true
flush()
assert(disconnected.pos == nil)
print("PASS: map guard, deferred teleport, fallback, disconnect; noclip enabled; no entities")
