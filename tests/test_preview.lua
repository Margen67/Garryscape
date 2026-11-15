-- SPDX-License-Identifier: AGPL-3.0-only
-- Exercise server lifecycle hooks without pretending to run the Source engine.
local script = "addons/garryscape/lua/autorun/server/garryscape_preview.lua"
local hooks, timers, entities, errors
local map, validModel, config

local vectorMeta = { __add = function(a, b)
    return setmetatable({x=a.x+b.x, y=a.y+b.y, z=a.z+b.z}, getmetatable(a))
end }
function Vector(x,y,z) return setmetatable({x=x,y=y,z=z}, vectorMeta) end
function Angle(p,y,r) return {p=p,y=y,r=r} end
angle_zero = Angle(0,0,0)
MOVETYPE_NOCLIP, MOVETYPE_NONE, SOLID_NONE = 8, 0, 0
function istable(value) return type(value) == "table" end
function isstring(value) return type(value) == "string" end
function IsValid(value) return type(value) == "table" and not value.removed end
function ErrorNoHalt(message) errors[#errors+1] = message end
hook = { Add = function(event, _, fn) hooks[event] = fn end }
timer = { Simple = function(delay, fn) assert(delay == 0); timers[#timers+1] = fn end }
game = { GetMap = function() return map end }
file = { Read = function(path, realm)
    assert(path == "data/garryscape/terrain_preview.json" and realm == "GAME")
    return "manifest"
end }
util = {
    JSONToTable = function(_) return config end,
    IsValidModel = function(_) return validModel end
}
local methods = {}
function methods:SetName(v) self.name = v end
function methods:SetModel(v) self.model = v end
function methods:SetPos(v) self.pos = v end
function methods:SetAngles(v) self.angles = v end
function methods:SetEyeAngles(v) self.eyeAngles = v end
function methods:SetMoveType(v) self.moveType = v end
function methods:SetSolid(v) self.solid = v end
function methods:DrawShadow(v) self.shadow = v end
function methods:SetKeyValue(k,v) self[k] = v end
function methods:Spawn() self.spawned = true end
function methods:Remove() self.removed = true end
local function entity()
    return setmetatable({}, {__index = methods})
end
ents = {
    Create = function(class)
        assert(class == "prop_dynamic_override")
        local ent = entity(); entities[#entities+1] = ent; return ent
    end,
    FindByName = function(name)
        local result = {}
        for _, ent in ipairs(entities) do
            if IsValid(ent) and ent.name == name then result[#result+1] = ent end
        end
        return result
    end
}
local function reset(whichMap, manifest)
    hooks, timers, entities, errors = {}, {}, {}, {}
    map, validModel, config = whichMap, true, manifest
    dofile(script)
end

reset("gm_construct", nil)
assert(next(hooks) == nil, "preview must not affect other maps")

reset("garryscape", {model="models/garryscape/530/regions/50_50.mdl", sourceBounds={{-100,-100,0},{100,100,370}}})
hooks.InitPostEntity()
local terrain = entities[1]
assert(terrain.spawned and terrain.model == config.model)
assert(terrain.pos.x == 0 and terrain.pos.y == 0 and terrain.pos.z == -12000)
assert(terrain.solid == SOLID_NONE and terrain.moveType == MOVETYPE_NONE)
assert(terrain.shadow == false)
hooks.PostCleanupMap()
assert(terrain.removed and #entities == 2 and entities[2].spawned)
local player = entity()
hooks.PlayerSpawn(player)
assert(player.pos == nil, "teleport must be deferred past PlayerSpawn")
for _, fn in ipairs(timers) do fn() end
assert(player.pos.x == 0 and player.pos.y == 0 and player.pos.z == -10430)
assert(player.moveType == MOVETYPE_NOCLIP)
assert(player.eyeAngles.p == 89 and player.eyeAngles.y == 0 and player.eyeAngles.r == 0)
assert(hooks.Think == nil and hooks.PostDrawOpaqueRenderables == nil, "no per-frame rendering hook")

reset("garryscape", nil)
validModel = false
hooks.InitPostEntity()
assert(#entities == 0 and #errors == 1, "missing model must produce a useful diagnostic")
validModel = true
hooks.InitPostEntity()
assert(entities[1].model == "models/garryscape/2009scape/regions/50_50.mdl")
local disconnected = entity()
hooks.PlayerSpawn(disconnected)
disconnected:Remove()
for _, fn in ipairs(timers) do fn() end
assert(disconnected.pos == nil)
print("PASS: map guard, native entity lifecycle, cleanup, deferred camera, missing-model diagnostic")
