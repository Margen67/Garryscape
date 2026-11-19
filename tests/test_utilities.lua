-- SPDX-License-Identifier: AGPL-3.0-only
local hooks, option, checkbox = {};
function CreateClientConVar(name, default, archive, userinfo)
    assert(name == "garryscape_ground_blending" and default == "1" and archive and not userinfo);
end

hook = {
    Add = function(event, id, fn)
        assert(event == "PopulateToolMenu" and id == "GarryscapeUtilities");
        hooks[event] = fn;
    end
};

spawnmenu = {
    AddToolCategory = function(tab, id, label) assert(tab == "Utilities" and id == "Garryscape" and label == "Garryscape"); end,
    AddToolMenuOption = function(tab, category, id, label, _a, _b, build)
        assert(tab == "Utilities" and category == "Garryscape" and id == "GarryscapeTerrain" and label == "Settings");
        option = build;
    end
};

dofile("addons/garryscape/lua/autorun/client/garryscape_utilities.lua");
assert(hooks.PopulateToolMenu);
hooks.PopulateToolMenu();
option({
    ClearControls = function() end,
    CheckBox = function(_, label, cv)
        assert(label == "Ground blending" and cv == "garryscape_ground_blending");
        checkbox = true;
    end,
    Help = function() end
});

assert(checkbox);
print("PASS: Utilities > Garryscape > Settings checkbox uses the archived blending convar");
