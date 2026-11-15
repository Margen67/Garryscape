-- SPDX-License-Identifier: AGPL-3.0-only
-- Automate the in-game preview using compiled Source assets only.
AddCSLuaFile("autorun/client/garryscape_shader.lua")
AddCSLuaFile("garryscape/atlas_shader.lua")
if game.GetMap():lower() ~= "garryscape" then return end

local origin = Vector(0, 0, -12000)
local model = "models/garryscape/2009scape/regions/50_50.mdl"
local position = Vector(0, 0, -10400)
local config = util.JSONToTable(file.Read("data/garryscape/terrain_preview.json", "GAME") or "")
if istable(config) and isstring(config.model) then model = config.model end
if istable(config) and istable(config.sourceBounds) and istable(config.sourceBounds[2]) then
    local height = tonumber(config.sourceBounds[2][3])
    if height then position = Vector(0, 0, -12000 + height + 1200) end
end

hook.Add("PlayerSpawn", "Garryscape.TerrainPreview", function(ply)
    timer.Simple(0, function()
        if not IsValid(ply) then return end
        ply:SetMoveType(MOVETYPE_NOCLIP)
        ply:SetPos(position)
        ply:SetEyeAngles(Angle(89, 0, 0))
    end)
end)

local function spawnTerrain()
    if not util.IsValidModel(model) then
        ErrorNoHalt("[Garryscape] Missing compiled terrain model: " .. model .. "\n")
        return
    end
    for _, ent in ipairs(ents.FindByName("garryscape_terrain_preview")) do ent:Remove() end
    local ent = ents.Create("prop_dynamic_override")
    if not IsValid(ent) then return end
    ent:SetName("garryscape_terrain_preview")
    ent:SetModel(model)
    ent:SetPos(origin)
    ent:SetAngles(Angle(0, 0, 0))
    ent:SetKeyValue("solid", "0")
    ent:Spawn()
    ent:SetSolid(SOLID_NONE)
    ent:SetMoveType(MOVETYPE_NONE)
    ent:DrawShadow(false)
end

hook.Add("InitPostEntity", "Garryscape.TerrainPreview", spawnTerrain)
hook.Add("PostCleanupMap", "Garryscape.TerrainPreview", spawnTerrain)
