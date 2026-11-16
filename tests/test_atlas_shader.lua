-- SPDX-License-Identifier: AGPL-3.0-only
-- Validate module/API wiring only; actual D3D shader execution needs Windows/EGSM.
local names={"PARAM_BASETEXTURE","SHADER_PARAM_TYPE_VEC4","PSREG_LIGHT_INFO_ARRAY",
"STDCONST_LIGHT_INFO","PSREG_FOG_PARAMS","STDCONST_FOG_PARAMS",
"PSREG_DIFFUSE_MODULATION","STDCONST_DIFFUSE_MODULATION","PSREG_AMBIENT_CUBE",
"STDCONST_AMBIENT_CUBE","PSREG_SHADER_CONTROLS","STDCONST_SHADER_CONTROLS",
"PSREG_EYEPOS_SPEC_EXPONENT","STDCONST_EYE_POS"}
for i,name in ipairs(names) do _G[name]=i end
MATERIAL_VAR_TRANSLUCENT=8
MATERIAL_VAR2_SUPPORTS_HW_SKINNING=1
MATERIAL_VAR2_LIGHTING_VERTEX_LIT=2
game={GetMap=function() return "garryscape" end}
EGSM={Version=2}
local compiled,shaders,errors={},{},{}
local callbacks={}
local values={mat_forceaniso=8,mat_filtertextures=1,mat_mipmaptextures=1,garryscape_ground_blending=1}
function GetConVar(name)
    if values[name]==nil then return nil end
    return {GetFloat=function() return values[name] end,GetBool=function() return values[name]>0 end}
end
function CreateClientConVar(name,default) values[name]=values[name] or tonumber(default);return GetConVar(name) end
math.Clamp=function(n,lo,hi) return math.max(lo,math.min(n,hi)) end
cvars={AddChangeCallback=function(name,callback,id)
    assert(values[name]~=nil)
    callbacks[name]=callback
end}
function ErrorNoHalt(message) errors[#errors+1]=message end
function include(path) return dofile("addons/garryscape/lua/"..path) end
shaderlib={
    CompileVertexShader=function(name,flags,source)
        assert(flags==0 and source:find("SkinPositionAndNormal",1,true))
        assert(not source:find("round(",1,true),"vertex positions must not be rounded")
        compiled[name]=source
    end,
    CompilePixelShader=function(name,flags,source)
        assert(flags==0 and source:find("tex2Dlod",1,true))
        assert(source:find("floor(i/dimensions)",1,true),"filter taps must wrap within the image")
        compiled[name]=source
    end,
    NewShader=function(name)
        local shader={parameters={},constants={}}
        function shader:SetVertexShader(value) assert(compiled[value]);self.vs=value end
        function shader:SetPixelShader(value) assert(compiled[value]);self.ps=value end
        function shader:BindTexture(slot,param) assert(slot==0 and param==PARAM_BASETEXTURE) end
        function shader:EnableFlashlightSupport(value) assert(value) end
        function shader:AddParam(param,kind,default)
            assert(kind==SHADER_PARAM_TYPE_VEC4 and type(default)=="string")
            self.parameters[#self.parameters+1]=param
            return #self.parameters
        end
        function shader:SetPixelShaderConstant(index,x,y,z,w)
            assert(w==0)
            if index==43 then self.filter=x;self.textureFilter=y;self.mipmaps=z
            elseif index==47 then self.blending=x
            elseif index==49 then self.water=x
            else error("unexpected uniform "..index) end
        end
        function shader:SetVertexShaderConstantFP(index,param) assert(index==48);self.mapping=self.parameters[param] end
        function shader:SetPixelShaderConstantFP(index,param) self.constants[index]=self.parameters[param] end
        function shader:SetPixelShaderStandardConstant(index,constant) assert(index and constant) end
        function shader:SetFlags(value) self.flags=value end
        function shader:SetFlags2(value) assert(value==3) end
        shaders[name]=shader
        return shader
    end
}
-- Shader setup must not add per-frame Lua rendering hooks.
hook={Add=function() error("native atlas shader must not install a Lua draw loop") end}
dofile("addons/garryscape/lua/autorun/client/garryscape_shader.lua")
assert(#errors==0,errors[1])
for _,name in ipairs({"GarryscapeAtlas","GarryscapeAtlasWater"}) do
    local shader=assert(shaders[name])
    assert(shader.constants[44]=="$atlasdimensions")
    assert(shader.constants[40]=="$atlasrect" and shader.constants[41]=="$atlasinfo" and shader.constants[42]=="$tint")
end
assert(shaders.GarryscapeAtlas.flags==0 and shaders.GarryscapeAtlasWater.flags==MATERIAL_VAR_TRANSLUCENT)
for _,shader in pairs(shaders) do assert(shader.filter==8) end
for _,setting in ipairs({0,1,2,4,8,16,32}) do
    values.mat_forceaniso=setting
    callbacks.mat_forceaniso("mat_forceaniso","8",tostring(setting))
    for _,shader in pairs(shaders) do
        assert(shader.filter==math.Clamp(setting,1,16))
    end
end
for _,name in ipairs({"mat_filtertextures","mat_mipmaptextures","garryscape_ground_blending"}) do
    for _,value in ipairs({0,1}) do
        values[name]=value;callbacks[name]()
        for _,shader in pairs(shaders) do
            if name=="mat_filtertextures" then assert(shader.textureFilter==value)
            elseif name=="mat_mipmaptextures" then assert(shader.mipmaps==value)
            else assert(shader.blending==value) end
        end
    end
end
assert(shaders.GarryscapeAtlas.water==0 and shaders.GarryscapeAtlasWater.water==1)
-- Reinitialization replaces the named callback and reads the current setting.
values.mat_forceaniso=4
include("garryscape/atlas_shader.lua")
for _,shader in pairs(shaders) do assert(shader.filter==4) end
EGSM=nil
shaderlib=nil
dofile("addons/garryscape/lua/autorun/client/garryscape_shader.lua")
assert(#errors==1 and errors[1]:find("requires EGSM",1,true))
print("PASS: shader API wiring, image-rectangle parameters, flashlight flags, missing-module diagnostic; no Lua draw loop")
