-- SPDX-License-Identifier: AGPL-3.0-only
-- Native Source assets only. This addon never reads RuneScape cache files.
-- Terrain preview has no collision; players are put in noclip to inspect it.
if game.GetMap():lower() ~= "garryscape" then return end

local origin = Vector(0, 0, -12000)
local model = "models/garryscape/2009scape/regions/50_50.mdl"
local cameraHeight = 1600
local configPath = "data/garryscape/terrain_preview.json"
local config = util.JSONToTable(file.Read(configPath, "GAME") or "")
if istable(config) and isstring(config.model) then
    model = config.model
    if istable(config.sourceBounds) and istable(config.sourceBounds[2]) then
        cameraHeight = (tonumber(config.sourceBounds[2][3]) or 400) + 1200
    end
end

local function placePlayer(ply)
    timer.Simple(0, function()
        if not IsValid(ply) then return end
        -- The user chose a camera above the terrain instead of Z=-13200.
        ply:SetMoveType(MOVETYPE_NOCLIP)
        ply:SetPos(origin + Vector(0, 0, cameraHeight))
        ply:SetEyeAngles(Angle(89, 0, 0))
    end)
end

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
    ent:SetAngles(angle_zero)
    ent:SetKeyValue("solid", "0")
    ent:Spawn()
    ent:SetSolid(SOLID_NONE)
    ent:SetMoveType(MOVETYPE_NONE)
    ent:DrawShadow(false)
end

hook.Add("InitPostEntity", "Garryscape.TerrainPreview", spawnTerrain)
hook.Add("PostCleanupMap", "Garryscape.TerrainPreview", spawnTerrain)
hook.Add("PlayerSpawn", "Garryscape.TerrainPreview", placePlayer)
