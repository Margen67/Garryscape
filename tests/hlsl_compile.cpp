// SPDX-License-Identifier: AGPL-3.0-or-later
// Optional native Microsoft HLSL compiler test helper. No renderer/GPU required.
// Windows: cl /EHsc /std:c++17 tests/hlsl_compile.cpp /Fe:hlsl_compile.exe
// Also builds as a 64-bit Winelib program with wineg++.
#define NOMINMAX
#include <windows.h>
#include <d3dcompiler.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cctype>
#include <fstream>
#include <sstream>
#include <string>

struct Includes : ID3DInclude {
    std::string root;
    HRESULT __stdcall Open(D3D_INCLUDE_TYPE, LPCSTR name, LPCVOID,
                           LPCVOID* data, UINT* length) override {
        std::ifstream file(root + "/" + name, std::ios::binary);
        if (!file) return E_FAIL;
        std::ostringstream stream;
        stream << file.rdbuf();
        const std::string text = stream.str();
        char* bytes = new char[text.size()];
        memcpy(bytes, text.data(), text.size());
        *data = bytes;
        *length = static_cast<UINT>(text.size());
        return S_OK;
    }
    HRESULT __stdcall Close(LPCVOID data) override {
        delete[] static_cast<const char*>(data);
        return S_OK;
    }
};

int main(int argc, char** argv) {
    if (argc < 4) {
        fprintf(stderr, "Usage: hlsl_compile source profile include-directory [all|lights] [mode]\n");
        return 2;
    }
    const char* dll = getenv("GARRYSCAPE_D3DCOMPILER_DLL");
    HMODULE library = LoadLibraryA(dll ? dll : "d3dcompiler_47.dll");
    if (!library) {
        fprintf(stderr, "Cannot load Microsoft compiler DLL: error %lu\n", GetLastError());
        return 2;
    }
    using Compile = HRESULT(WINAPI*)(LPCVOID, SIZE_T, LPCSTR, const D3D_SHADER_MACRO*,
        ID3DInclude*, LPCSTR, LPCSTR, UINT, UINT, ID3DBlob**, ID3DBlob**);
    using Disassemble = HRESULT(WINAPI*)(LPCVOID, SIZE_T, UINT, LPCSTR, ID3DBlob**);
    auto compile = reinterpret_cast<Compile>(GetProcAddress(library, "D3DCompile"));
    auto disassemble = reinterpret_cast<Disassemble>(GetProcAddress(library, "D3DDisassemble"));
    if (!compile) return 2;
    std::ifstream file(argv[1], std::ios::binary);
    if (!file) { fprintf(stderr, "Cannot read %s\n", argv[1]); return 2; }
    std::ostringstream stream;
    stream << file.rdbuf();
    const std::string source = stream.str();
    Includes includes;
    includes.root = argv[3];
    const bool vertex = argv[2][0] == 'v';
    const bool all = argc > 4 && strcmp(argv[4], "all") == 0;
    int failures = 0;
    for (int mode = 0; mode < (all ? 2 : 1); ++mode) {
        for (int lights = 0; lights < (all ? 5 : 1); ++lights) {
            std::string n = all ? std::to_string(lights) : (argc > 4 ? argv[4] : "0");
            std::string m = all ? std::to_string(mode) : (argc > 5 ? argv[5] : "0");
            const D3D_SHADER_MACRO macros[] = {
                {"NUM_LIGHTS", n.c_str()}, {"SKINNING", m.c_str()},
                {"FLASHLIGHT", m.c_str()}, {"X64", "1"}, {"COMPRESSED_VERTS", "1"},
                {vertex ? "SHADER_MODEL_VS_3_0" : "SHADER_MODEL_PS_3_0", "1"},
                {"SCR_W", "1920"}, {"SCR_H", "1080"},
                {"SCR_S", "float2(SCR_W,SCR_H)"}, {nullptr, nullptr}
            };
            ID3DBlob *code = nullptr, *errors = nullptr;
            // Match EGSM's default compile flags and all ten stage variants.
            HRESULT result = compile(source.data(), source.size(), argv[1], macros,
                &includes, "main", argv[2], 0, 0, &code, &errors);
            if (FAILED(result)) {
                printf("FAIL %s lights=%s mode=%s\n", argv[2], n.c_str(), m.c_str());
                if (errors) fwrite(errors->GetBufferPointer(), 1, errors->GetBufferSize(), stdout);
                ++failures;
            } else {
                int highest = -1;
                ID3DBlob* assembly = nullptr;
                if (disassemble && SUCCEEDED(disassemble(code->GetBufferPointer(), code->GetBufferSize(), 0, nullptr, &assembly))) {
                    std::string text(static_cast<const char*>(assembly->GetBufferPointer()), assembly->GetBufferSize());
                    for (size_t i = 0; i + 1 < text.size(); ++i) {
                        if (text[i] != 'r' || !isdigit(static_cast<unsigned char>(text[i + 1]))) continue;
                        if (i && (isalnum(static_cast<unsigned char>(text[i - 1])) || text[i - 1] == '_')) continue;
                        int reg = atoi(text.c_str() + i + 1);
                        if (reg > highest) highest = reg;
                    }
                    assembly->Release();
                }
                printf("PASS %s lights=%s mode=%s bytes=%zu highest_temp=r%d\n",
                       argv[2], n.c_str(), m.c_str(), code->GetBufferSize(), highest);
            }
            if (errors) errors->Release();
            if (code) code->Release();
            fflush(stdout);
        }
    }
    FreeLibrary(library);
    return failures ? 1 : 0;
}
