-- SPDX-License-Identifier: AGPL-3.0-only
-- EGSM must be installed in the menu realm before client materials load.
if not EGSM or not shaderlib then
    if game.GetMap():lower() == "garryscape" then
        ErrorNoHalt("[Garryscape] The native atlas shader requires EGSM (x86-64). " ..
            "See README.md; stock materials cannot repeat individual atlas entries.\n");
    end
    return;
end

local ok, err = pcall(include, "garryscape/atlas_shader.lua");
if not ok then ErrorNoHalt("[Garryscape] Atlas shader initialization failed: " .. tostring(err) .. "\n"); end
