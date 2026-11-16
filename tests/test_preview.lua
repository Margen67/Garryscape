-- SPDX-License-Identifier: AGPL-3.0-only
-- Exercise teleport hooks without pretending to run the Source engine.
local script = "addons/garryscape/lua/autorun/server/garryscape_preview.lua"
local hooks, timers, map, config, entities, errors, validModel
function AddCSLuaFile(path) assert(path=="autorun/client/garryscape_utilities.lua" or path=="autorun/client/garryscape_shader.lua" or path=="garryscape/atlas_shader.lua") end
function Vector(x,y,z) return {x=x,y=y,z=z} end
function Angle(p,y,r) return {p=p,y=y,r=r} end
function isstring(value) return type(value) == "string" end
function ErrorNoHalt(value) errors[#errors+1] = value end
function istable(value) return type(value) == "table" end
function IsValid(value) return type(value) == "table" and not value.removed end
hook = { Add = function(event, _, fn) hooks[event] = fn end }
timer = { Simple = function(delay, fn) assert(delay == 0); timers[#timers+1] = fn end }
game = { GetMap = function() return map end }
file = { Read = function(path, realm)
    assert(path == "data/garryscape/terrain_preview.json" and realm == "GAME")
    return "manifest"
end }
util = { JSONToTable = function(_) return config end, IsValidModel = function(_) return validModel end }
MOVETYPE_NOCLIP = 8
MOVETYPE_NONE, SOLID_NONE = 0, 0
ents = {
    Create = function(class)
        assert(class == "prop_dynamic_override")
        local ent = {
            SetName = function(self,v) self.name=v end,
            SetModel = function(self,v) self.model=v end,
            SetPos = function(self,v) self.pos=v end,
            SetAngles = function(self,v) self.angles=v end,
            SetKeyValue = function(self,k,v) self[k]=v end,
            Spawn = function(self) self.spawned=true end,
            SetSolid = function(self,v) self.solid=v end,
            SetMoveType = function(self,v) self.moveType=v end,
            DrawShadow = function(self,v) self.shadow=v end,
            Remove = function(self) self.removed=true end
        }
        entities[#entities+1]=ent
        return ent
    end,
    FindByName = function(name)
        local result={}
        for _,ent in ipairs(entities) do
            if IsValid(ent) and ent.name==name then result[#result+1]=ent end
        end
        return result
    end
}
local function player()
    return {
        SetPos = function(self, value) self.pos = value end,
        SetEyeAngles = function(self, value) self.eyeAngles = value end,
        SetMoveType = function(self, value) self.moveType = value end
    }
end
local function reset(whichMap, manifest)
    hooks, timers, map, config = {}, {}, whichMap, manifest
    entities, errors, validModel = {}, {}, true
    dofile(script)
end
local function flush()
    for _, fn in ipairs(timers) do fn() end
    timers = {}
end

reset("gm_construct", nil)
assert(next(hooks) == nil, "preview must not affect other maps")

reset("garryscape", {model="models/garryscape/530/regions/50_50.mdl",sourceBounds={{-100,-100,0},{100,100,370}}})
hooks.InitPostEntity()
local terrain=entities[1]
assert(terrain.spawned and terrain.model==config.model)
assert(terrain.pos.x==0 and terrain.pos.y==0 and terrain.pos.z==-12000)
assert(terrain.solid==SOLID_NONE and terrain.moveType==MOVETYPE_NONE)
hooks.PostCleanupMap()
assert(terrain.removed and #entities==2 and entities[2].spawned)
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
validModel=false
hooks.InitPostEntity()
assert(#entities==0 and #errors==1)
validModel=true
hooks.InitPostEntity()
assert(entities[1].model=="models/garryscape/2009scape/regions/50_50.mdl")
print("PASS: terrain spawning/cleanup, map guard, missing-model diagnostic, teleport and noclip")
