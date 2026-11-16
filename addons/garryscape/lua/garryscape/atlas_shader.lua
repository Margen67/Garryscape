-- SPDX-License-Identifier: AGPL-3.0-only
-- EGSM's public shader API supplies native model transforms and Source lighting.
-- No cache decoding, vertex deformation, or per-frame Lua work.
shaderlib.CompileVertexShader("GarryscapeAtlasVS", 0, [==[
#include "common_vs_fxc.h"
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
]==])
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
struct PS_INPUT {
    float3 screen : VPOS;
    float2 uv : TEXCOORD0;
    float4 lightAtten : TEXCOORD1;
    float3 normal : TEXCOORD2;
    float3 worldPos : TEXCOORD3;
    float4 projPos : TEXCOORD4;
};
float4 SampleLevel(float2 uv,float level) {
    float divisor=exp2(level);
    float2 dimensions=Info.yz/divisor;
    float2 origin=Rect.xy/divisor;
    float2 atlasSize=max(AtlasDimensions.xy/divisor,float2(1,1));
    float2 p=frac(uv)*dimensions-0.5;
    float2 i=floor(p),w=frac(p);
    float2 a=i-floor(i/dimensions)*dimensions;
    float2 b=(i+1)-floor((i+1)/dimensions)*dimensions;
    float4 aa=tex2Dlod(Atlas,float4((origin+a+0.5)/atlasSize,0,level));
    float4 ba=tex2Dlod(Atlas,float4((origin+float2(b.x,a.y)+0.5)/atlasSize,0,level));
    float4 ab=tex2Dlod(Atlas,float4((origin+float2(a.x,b.y)+0.5)/atlasSize,0,level));
    float4 bb=tex2Dlod(Atlas,float4((origin+b+0.5)/atlasSize,0,level));
    return lerp(lerp(aa,ba,w.x),lerp(ab,bb,w.x),w.y);
}
float4 SampleNative(float2 uv) {
    if (Info.y==0) return float4(1,1,1,1);
    float2 dx=ddx(uv)*Info.yz,dy=ddy(uv)*Info.yz;
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
    float limit=clamp(Filtering.x,1,16);
    float width=max(max(sqrt(minor2),major/limit),1.0);
    int taps=(int)clamp(ceil(major/width),1,limit);
    // Retain the original isotropic footprint when anisotropy is disabled.
    float level=clamp(log2(width),0,Info.w);
    float lo=floor(level),hi=min(lo+1,Info.w);
    float4 result=float4(0,0,0,0);
    [loop] for (int tap=0;tap<taps;tap++) {
        float offset=((tap+0.5)/taps-0.5)*major;
        float2 sampleUV=uv+axis*offset/Info.yz;
        result+=lerp(SampleLevel(sampleUV,lo),SampleLevel(sampleUV,hi),frac(level));
    }
    return result/taps;
}
float4 main(PS_INPUT i) : COLOR {
    float4 base=SampleNative(i.uv)*Tint;
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
]==])
local atlasShaders={}
local function updateFiltering(_,_,value)
    local convar=GetConVar("mat_forceaniso")
    local amount=math.Clamp(tonumber(value) or (convar and convar:GetFloat()) or 1,1,16)
    for _,shader in ipairs(atlasShaders) do
        shader:SetPixelShaderConstant(43,amount,0,0,0)
    end
end
for _, water in ipairs({false,true}) do
    local shader=shaderlib.NewShader(water and "GarryscapeAtlasWater" or "GarryscapeAtlas")
    atlasShaders[#atlasShaders+1]=shader
    shader:SetVertexShader("GarryscapeAtlasVS")
    shader:SetPixelShader("GarryscapeAtlasPS")
    shader:BindTexture(0,PARAM_BASETEXTURE)
    shader:EnableFlashlightSupport(true)
    shader:SetPixelShaderConstantFP(40,shader:AddParam("$atlasrect",SHADER_PARAM_TYPE_VEC4,"[0 0 128 128]"))
    shader:SetPixelShaderConstantFP(41,shader:AddParam("$atlasinfo",SHADER_PARAM_TYPE_VEC4,"[128 128 128 7]"))
    shader:SetPixelShaderConstantFP(44,shader:AddParam("$atlasdimensions",SHADER_PARAM_TYPE_VEC4,"[128 128 0 0]"))
    shader:SetPixelShaderConstantFP(42,shader:AddParam("$tint",SHADER_PARAM_TYPE_VEC4,"[1 1 1 1]"))
    for _, pair in ipairs({
        {PSREG_LIGHT_INFO_ARRAY,STDCONST_LIGHT_INFO},
        {PSREG_FOG_PARAMS,STDCONST_FOG_PARAMS},
        {PSREG_DIFFUSE_MODULATION,STDCONST_DIFFUSE_MODULATION},
        {PSREG_AMBIENT_CUBE,STDCONST_AMBIENT_CUBE},
        {PSREG_SHADER_CONTROLS,STDCONST_SHADER_CONTROLS},
        {PSREG_EYEPOS_SPEC_EXPONENT,STDCONST_EYE_POS}
    }) do shader:SetPixelShaderStandardConstant(pair[1],pair[2]) end
    shader:SetFlags(water and MATERIAL_VAR_TRANSLUCENT or 0)
    shader:SetFlags2(MATERIAL_VAR2_SUPPORTS_HW_SKINNING+MATERIAL_VAR2_LIGHTING_VERTEX_LIT)
end

-- Update only when the setting changes, never from a rendering hook.
updateFiltering()
cvars.AddChangeCallback("mat_forceaniso",updateFiltering,"GarryscapeAtlasFiltering")
