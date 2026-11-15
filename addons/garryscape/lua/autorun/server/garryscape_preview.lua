-- SPDX-License-Identifier: AGPL-3.0-only
-- Only place players for the garryscape map preview. Assets are placed separately.
if game.GetMap():lower() ~= "garryscape" then return end

local position = Vector(0, 0, -10400)
local config = util.JSONToTable(file.Read("data/garryscape/terrain_preview.json", "GAME") or "")
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
