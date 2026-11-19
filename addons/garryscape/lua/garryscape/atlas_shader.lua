-- SPDX-License-Identifier: AGPL-3.0-only
-- EGSM's public shader API supplies native model transforms and Source lighting.
-- No cache decoding, vertex deformation, or per-frame Lua work.
shaderlib.CompileVertexShader("GarryscapeAtlasVS", 0, [==[
#include "common_vs_fxc.h"
const float4 RegionMapping : register(c48);
struct VS_INPUT {
    float4 pos : POSITION;
    float4 weights : BLENDWEIGHT;
    float4 bones : BLENDINDICES;
    float4 normal : NORMAL;
    float2 uv : TEXCOORD0;
};
struct VS_OUTPUT {
    float4 pos : POSITION;
    float2 uv : TEXCOORD0;
    float4 lightAtten : TEXCOORD1;
    float3 normal : TEXCOORD2;
    float3 worldPos : TEXCOORD3;
    float4 projPos : TEXCOORD4;
    float3 terrain : TEXCOORD5;
};
VS_OUTPUT main(VS_INPUT v) {
    VS_OUTPUT o=(VS_OUTPUT)0;
    float3 n;
    DecompressVertex_Normal(v.normal,n);
    float3 p,wn;
    SkinPositionAndNormal(SKINNING,v.pos,n,v.weights,v.bones,p,wn);
    o.pos=mul(float4(p,1),cViewProj);
    o.projPos=o.pos;
    o.uv=v.uv;
    // Source UV carries a precomputed blend address and terrain layer. Repeat UVs are recovered
    // from the unchanged local position, not the entity's world placement.
    o.terrain=float3(v.pos.xy*RegionMapping.x+RegionMapping.y,v.uv.y);
    o.normal=normalize(wn);
    o.worldPos=p;
#if NUM_LIGHTS > 0
    o.lightAtten.x=GetVertexAttenForLight(p,0,false);
#endif
#if NUM_LIGHTS > 1
    o.lightAtten.y=GetVertexAttenForLight(p,1,false);
#endif
#if NUM_LIGHTS > 2
    o.lightAtten.z=GetVertexAttenForLight(p,2,false);
#endif
#if NUM_LIGHTS > 3
    o.lightAtten.w=GetVertexAttenForLight(p,3,false);
#endif
    return o;
}
]==]);
shaderlib.CompilePixelShader("GarryscapeAtlasPS", 0, [==[
#include "common_fxc.h"
#include "shader_constant_register_map.h"
#include "common_vertexlitgeneric_dx9.h"
#include "common_flashlight_fxc.h"
sampler Atlas : register(s0);
sampler ShadowDepthSampler : register(s4);
sampler NormalizeRandRotSampler : register(s5);
sampler FlashlightSampler : register(s6);
const float4 Modulation : register(PSREG_DIFFUSE_MODULATION);
const float3 AmbientCube[6] : register(PSREG_AMBIENT_CUBE);
const float4 Eye : register(PSREG_EYEPOS_SPEC_EXPONENT);
const float4 Fog : register(PSREG_FOG_PARAMS);
const float4 FlashAtten : register(PSREG_FLASHLIGHT_ATTENUATION);
const float4 FlashPos : register(PSREG_FLASHLIGHT_POSITION_RIM_BOOST);
const float4x4 FlashMatrix : register(PSREG_FLASHLIGHT_TO_WORLD_TEXTURE);
PixelShaderLightInfo Lights[3] : register(PSREG_LIGHT_INFO_ARRAY);
const float4 g_ShaderControls : register(PSREG_SHADER_CONTROLS);
const float4 Rect : register(c40);
const float4 Info : register(c41);
const float4 Tint : register(c42);
const float4 Filtering : register(c43);
const float4 AtlasDimensions : register(c44);
const float4 Control : register(c45);
const float4 Mapping : register(c46);
const float4 BlendingEnabled : register(c47);
const float4 TextureScale : register(c48);
const float4 Water : register(c49);
struct PS_INPUT {
    float3 screen : VPOS;
    float2 uv : TEXCOORD0;
    float4 lightAtten : TEXCOORD1;
    float3 normal : TEXCOORD2;
    float3 worldPos : TEXCOORD3;
    float4 projPos : TEXCOORD4;
    float3 terrain : TEXCOORD5;
};
float4 SampleLevel(float2 uv,float level,float4 rect) {
    float divisor=exp2(level);
    float2 dimensions=rect.zw/divisor;
    float2 origin=rect.xy/divisor;
    float2 atlasSize=max(AtlasDimensions.xy/divisor,float2(1,1));
    if (Filtering.y<0.5) {
        float2 pixel=min(floor(frac(uv)*dimensions),dimensions-1);
        return tex2Dlod(Atlas,float4((origin+pixel+0.5)/atlasSize,0,level));
    }
    float2 p=frac(uv)*dimensions-0.5;
    float2 i=floor(p),w=frac(p);
    float2 a=i-floor(i/dimensions)*dimensions;
    float2 b=(i+1)-floor((i+1)/dimensions)*dimensions;
    // Accumulate taps immediately instead of keeping four float4 samples live.
    float4 result=tex2Dlod(Atlas,float4((origin+a+0.5)/atlasSize,0,level))*(1-w.x)*(1-w.y);
    result+=tex2Dlod(Atlas,float4((origin+float2(b.x,a.y)+0.5)/atlasSize,0,level))*w.x*(1-w.y);
    result+=tex2Dlod(Atlas,float4((origin+float2(a.x,b.y)+0.5)/atlasSize,0,level))*(1-w.x)*w.y;
    result+=tex2Dlod(Atlas,float4((origin+b+0.5)/atlasSize,0,level))*w.x*w.y;
    return result;
}
float4 SampleNative(float2 uv,float4 rect,float2 du,float2 dv) {
    if (rect.z==0) return float4(1,1,1,1);
    float2 dx=du*rect.zw,dy=dv*rect.zw;
    // Principal axes of the pixel footprint in native texture texels.
    float xx=dx.x*dx.x+dy.x*dy.x;
    float yy=dx.y*dx.y+dy.y*dy.y;
    float xy=dx.x*dx.y+dy.x*dy.y;
    float spread=sqrt(max((xx-yy)*(xx-yy)+4*xy*xy,0));
    float major2=max(0.5*(xx+yy+spread),0);
    float minor2=max(0.5*(xx+yy-spread),0);
    float major=sqrt(major2);
    float2 axis=float2(xy,major2-xx);
    if (dot(axis,axis)<0.000001) axis=xx>=yy ? float2(1,0) : float2(0,1);
    else axis=normalize(axis);
    float limit=Filtering.y>0.5 ? clamp(Filtering.x,1,16) : 1;
    float width=max(max(sqrt(minor2),major/limit),1.0);
    int taps=(int)clamp(ceil(major/width),1,limit);
    // Retain the original isotropic footprint when anisotropy is disabled.
    float level=Filtering.z>0.5 ? clamp(log2(width),0,log2(rect.z)) : 0;
    if (Filtering.y<0.5) level=floor(level+0.5);
    float lo=floor(level),blend=frac(level);
    // One sample loop for both mip levels keeps the SM3 register footprint low
    // and avoids sampling a second mip when its contribution would be zero.
    int mips=blend>0 ? 2 : 1;
    float2 stride=axis*major/(rect.zw*taps);
    float2 first=uv+stride*(0.5-0.5*taps);
    float4 result=0;
    [loop] for (int sampleIndex=0;sampleIndex<taps*mips;sampleIndex++) {
        int tap=sampleIndex/mips;
        int high=sampleIndex-tap*mips;
        result+=SampleLevel(first+stride*tap,lo+high,rect)*(high>0 ? blend : 1-blend);
    }
    return result/taps;
}
float4 ReadControl(float index) {
    float2 pixel=float2(index-floor(index/Control.z)*Control.z,floor(index/Control.z));
    float4 value=tex2Dlod(Atlas,float4((Control.xy+pixel+0.5)/AtlasDimensions.xy,0,0));
    // EGSM enables sRGB reads on the albedo sampler. Undo that conversion for
    // control bytes only; native image samples stay in the engine's linear space.
    value.rgb=lerp(value.rgb*12.92,1.055*pow(max(value.rgb,0),1.0/2.4)-0.055,
        step(0.0031308,value.rgb));
    return floor(saturate(value)*255+0.5);
}
float Address(float3 v) {return dot(v,float3(1,256,65536));}
float2 Pair(float4 v) {return v.xz+v.yw*256;}
float Mask(float2 p,float4 a,float4 b,float4 c) {
    float2 ab=b.xy-a.xy,ac=c.xy-a.xy,q=p-a.xy;
    float det=ab.x*ac.y-ab.y*ac.x;
    if (abs(det)<0.5) return 0;
    float u=(q.x*ac.y-q.y*ac.x)/det;
    float v=(ab.x*q.y-ab.y*q.x)/det;
    if (min(min(u,v),1-u-v)<-0.00001) return 0;
    return saturate((a.z*(1-u-v)+b.z*u+c.z*v)/255);
}
// Feed the base material and blended layers through one sampler call site.
// Keeping a separate base sampler live across the blend loop exhausts SM3 temps.
float4 TerrainColor(float3 terrain,float address,float2 dx,float2 dy) {
    float4 result=0;
    float2 p=terrain.xy-clamp(floor(terrain.xy/128),0,63)*128;
    int count=0;
    address=floor(address+0.5);
    if (BlendingEnabled.x>0.5 && Water.x<0.5 && Control.z>=2 && address>=1) {
        count=(int)ReadControl(address).x;
        address+=1;
    }
    [loop] for (int layer=-1;layer<count;layer++) {
        float4 rect=Rect;
        float scale=TextureScale.x;
        float descriptor=-1;
        float opacity=1;
        if (layer>=0) {
            float4 header=ReadControl(address);
            descriptor=Address(header.xyz);
            int triangles=(int)header.w;
            address+=1;
            opacity=0;
            [loop] for (int ti=0;ti<triangles;ti++) {
                opacity=max(opacity,Mask(p,ReadControl(address),ReadControl(address+1),ReadControl(address+2)));
                address+=3;
            }
            if (opacity>0) {
                rect=float4(Pair(ReadControl(descriptor)),Pair(ReadControl(descriptor+1)));
                scale=Pair(ReadControl(descriptor+4)).x;
            }
        }
        if (opacity>0) {
            float2 uv=terrain.xy/scale+frac(Mapping.zw/scale);
            float4 color=SampleNative(uv,rect,dx/scale,dy/scale);
            if (descriptor<0) color*=Tint;
            else color*=float4(Pair(ReadControl(descriptor+2)),Pair(ReadControl(descriptor+3)))/8192;
            result=lerp(result,color,opacity);
        }
    }
    return result;
}
float4 main(PS_INPUT i) : COLOR {
    // Derivatives are evaluated before divergent mask loops; explicit LOD
    // sampling inside those loops remains valid on Shader Model 3 hardware.
    float2 dx=ddx(i.terrain.xy),dy=ddy(i.terrain.xy);
    float4 base=TerrainColor(i.terrain,i.uv.x,dx,dy);
    float3 lighting=float3(0,0,0);
    float3 normal=normalize(i.normal);
#if FLASHLIGHT || X64
    float4 flashlightPosition=mul(float4(i.worldPos,1),FlashMatrix);
    lighting=DoFlashlight(FlashPos.xyz,i.worldPos,flashlightPosition,normal,
        FlashAtten.xyz,FlashAtten.w,FlashlightSampler,ShadowDepthSampler,
        FlashlightSampler,0,false,false,i.screen,false);
#endif
#if !FLASHLIGHT || X64
    lighting+=PixelShaderDoLighting(i.worldPos,normal,float3(0,0,0),false,true,
        i.lightAtten,AmbientCube,NormalizeRandRotSampler,NUM_LIGHTS,Lights,false,
        false,1.0,false,Atlas);
#endif
    float fog=CalcPixelFogFactor(g_fPixelFogType,Fog,Eye.z,i.projPos.z,i.projPos.z);
    return FinalOutput(float4(base.rgb*Modulation.rgb*lighting,base.a*Modulation.a),
        fog,g_fPixelFogType,TONEMAP_SCALE_LINEAR,g_fWriteDepthToAlpha,i.projPos.z);
}
]==]);
local atlasShaders = {};
local function updateFiltering()
    local function setting(name, fallback)
        local cv = GetConVar(name);
        return cv and cv:GetFloat() or fallback;
    end

    local amount = math.Clamp(setting("mat_forceaniso", 1), 1, 16);
    local filter = setting("mat_filtertextures", 1) > 0 and 1 or 0;
    local mips = setting("mat_mipmaptextures", 1) > 0 and 1 or 0;
    for _, shader in ipairs(atlasShaders) do
        shader:SetPixelShaderConstant(43, amount, filter, mips, 0);
    end
end

local blending = GetConVar("garryscape_ground_blending") or CreateClientConVar("garryscape_ground_blending", "1", true, false, "Blend neighboring RuneScape terrain materials");
local function updateBlending()
    for _, shader in ipairs(atlasShaders) do
        shader:SetPixelShaderConstant(47, blending:GetBool() and 1 or 0, 0, 0, 0);
    end
end

for _, water in ipairs({false, true}) do
    local shader = shaderlib.NewShader(water and "GarryscapeAtlasWater" or "GarryscapeAtlas");
    atlasShaders[#atlasShaders + 1] = shader;
    shader:SetVertexShader("GarryscapeAtlasVS");
    shader:SetPixelShader("GarryscapeAtlasPS");
    shader:BindTexture(0, PARAM_BASETEXTURE);
    shader:EnableFlashlightSupport(true);
    shader:SetPixelShaderConstantFP(40, shader:AddParam("$atlasrect", SHADER_PARAM_TYPE_VEC4, "[0 0 128 128]"));
    shader:SetPixelShaderConstantFP(41, shader:AddParam("$atlasinfo", SHADER_PARAM_TYPE_VEC4, "[128 128 128 7]"));
    local mapping = shader:AddParam("$regionmapping", SHADER_PARAM_TYPE_VEC4, "[1 4096 0 0]");
    shader:SetVertexShaderConstantFP(48, mapping);
    shader:SetPixelShaderConstantFP(46, mapping);
    shader:SetPixelShaderConstantFP(45, shader:AddParam("$blendcontrol", SHADER_PARAM_TYPE_VEC4, "[0 0 0 0]"));
    shader:SetPixelShaderConstantFP(48, shader:AddParam("$texturescale", SHADER_PARAM_TYPE_VEC4, "[128 0 0 0]"));
    shader:SetPixelShaderConstant(49, water and 1 or 0, 0, 0, 0);
    shader:SetPixelShaderConstantFP(44, shader:AddParam("$atlasdimensions", SHADER_PARAM_TYPE_VEC4, "[128 128 0 0]"));
    shader:SetPixelShaderConstantFP(42, shader:AddParam("$tint", SHADER_PARAM_TYPE_VEC4, "[1 1 1 1]"));
    for _, pair in ipairs({
        {PSREG_LIGHT_INFO_ARRAY, STDCONST_LIGHT_INFO},
        {PSREG_FOG_PARAMS, STDCONST_FOG_PARAMS},
        {PSREG_DIFFUSE_MODULATION, STDCONST_DIFFUSE_MODULATION},
        {PSREG_AMBIENT_CUBE, STDCONST_AMBIENT_CUBE},
        {PSREG_SHADER_CONTROLS, STDCONST_SHADER_CONTROLS},
        {PSREG_EYEPOS_SPEC_EXPONENT, STDCONST_EYE_POS},
    }) do
        shader:SetPixelShaderStandardConstant(pair[1], pair[2]);
    end

    shader:SetFlags(water and MATERIAL_VAR_TRANSLUCENT or 0);
    shader:SetFlags2(MATERIAL_VAR2_SUPPORTS_HW_SKINNING + MATERIAL_VAR2_LIGHTING_VERTEX_LIT);
end

-- Update only when the setting changes, never from a rendering hook.
updateFiltering();
for _, name in ipairs({"mat_forceaniso", "mat_filtertextures", "mat_mipmaptextures"}) do
    cvars.AddChangeCallback(name, updateFiltering, "GarryscapeAtlasFiltering");
end

updateBlending();
cvars.AddChangeCallback("garryscape_ground_blending", updateBlending, "GarryscapeGroundBlending");
