// SPDX-License-Identifier: AGPL-3.0-or-later
//! Standalone Windows test helper for Microsoft's D3DCompile.

use std::{
    collections::HashMap,
    env,
    ffi::{CStr, CString, OsStr, c_char, c_void},
    fs,
    io::{self, Write as _},
    os::windows::ffi::OsStrExt as _,
    path::{Path, PathBuf},
    process::ExitCode,
    ptr,
};

type HResult = i32;
const E_FAIL: HResult = 0x8000_4005_u32 as i32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

struct Library(*mut c_void);

impl Library {
    fn load(path: &OsStr) -> io::Result<Self> {
        let wide: Vec<u16> = path.encode_wide().chain([0]).collect();
        // The terminated UTF-16 input lives through the call.
        let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn symbol(&self, name: &CStr) -> *mut c_void {
        // The handle and terminated symbol name are both valid here.
        unsafe { GetProcAddress(self.0, name.as_ptr()) }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // This object owns the loader reference.
        unsafe { FreeLibrary(self.0) };
    }
}

#[repr(C)]
struct Macro {
    name: *const c_char,
    value: *const c_char,
}

impl Macro {
    fn new(name: &CStr, value: &CStr) -> Self {
        Self {
            name: name.as_ptr(),
            value: value.as_ptr(),
        }
    }
}

// ID3DInclude is a two-method interface with no IUnknown prefix.
#[repr(C)]
struct IncludeVTable {
    open: unsafe extern "system" fn(
        *mut Includes,
        u32,
        *const c_char,
        *const c_void,
        *mut *const c_void,
        *mut u32,
    ) -> HResult,
    close: unsafe extern "system" fn(*mut Includes, *const c_void) -> HResult,
}

#[repr(C)]
struct Includes {
    vtable: &'static IncludeVTable,
    root: PathBuf,
    buffers: HashMap<*const c_void, Vec<u8>>,
}

static INCLUDE_VTABLE: IncludeVTable = IncludeVTable {
    open: open_include,
    close: close_include,
};

unsafe extern "system" fn open_include(
    this: *mut Includes,
    _kind: u32,
    name: *const c_char,
    _parent: *const c_void,
    data: *mut *const c_void,
    length: *mut u32,
) -> HResult {
    if name.is_null() || data.is_null() || length.is_null() {
        return E_FAIL;
    }
    // D3DCompile calls the interface synchronously with a C string and output pointers.
    let (includes, name) = unsafe { (&mut *this, CStr::from_ptr(name)) };
    let Ok(name) = name.to_str() else {
        return E_FAIL;
    };
    let Ok(mut bytes) = fs::read(includes.root.join(name)) else {
        return E_FAIL;
    };
    let Ok(size) = u32::try_from(bytes.len()) else {
        return E_FAIL;
    };
    // Give even an empty include its own allocation until Close.
    bytes.push(0);
    let address = bytes.as_ptr().cast();
    includes.buffers.insert(address, bytes);
    unsafe {
        data.write(address);
        length.write(size);
    }
    0
}

unsafe extern "system" fn close_include(this: *mut Includes, data: *const c_void) -> HResult {
    // The compiler passes back the pointer from Open.
    if unsafe { &mut *this }.buffers.remove(&data).is_some() {
        0
    } else {
        E_FAIL
    }
}

#[repr(C)]
struct Blob {
    vtable: *const BlobVTable,
}

// ID3DBlob inherits the three IUnknown methods.
#[repr(C)]
struct BlobVTable {
    query_interface:
        unsafe extern "system" fn(*mut Blob, *const c_void, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut Blob) -> u32,
    release: unsafe extern "system" fn(*mut Blob) -> u32,
    buffer: unsafe extern "system" fn(*mut Blob) -> *const u8,
    size: unsafe extern "system" fn(*mut Blob) -> usize,
}

struct OwnedBlob(ptr::NonNull<Blob>);

impl OwnedBlob {
    fn from_output(raw: *mut Blob) -> Option<Self> {
        ptr::NonNull::new(raw).map(Self)
    }

    fn bytes(&self) -> &[u8] {
        let raw = self.0.as_ptr();
        // The blob owns the reported buffer for the lifetime of this reference.
        unsafe {
            let vtable = &*(*raw).vtable;
            let size = (vtable.size)(raw);
            if size == 0 {
                &[]
            } else {
                std::slice::from_raw_parts((vtable.buffer)(raw), size)
            }
        }
    }
}

impl Drop for OwnedBlob {
    fn drop(&mut self) {
        let raw = self.0.as_ptr();
        // Release the reference returned by the compiler.
        unsafe { ((*(*raw).vtable).release)(raw) };
    }
}

type Compile = unsafe extern "system" fn(
    *const u8,
    usize,
    *const c_char,
    *const Macro,
    *mut Includes,
    *const c_char,
    *const c_char,
    u32,
    u32,
    *mut *mut Blob,
    *mut *mut Blob,
) -> HResult;
type Disassemble =
    unsafe extern "system" fn(*const u8, usize, u32, *const c_char, *mut *mut Blob) -> HResult;

fn highest_temp(assembly: &[u8]) -> i32 {
    let mut highest = -1;
    for (index, pair) in assembly.windows(2).enumerate() {
        if pair[0] != b'r' || !pair[1].is_ascii_digit() {
            continue;
        }
        if index != 0
            && (assembly[index - 1].is_ascii_alphanumeric() || assembly[index - 1] == b'_')
        {
            continue;
        }
        let number = assembly[index + 1..]
            .iter()
            .take_while(|digit| digit.is_ascii_digit())
            .fold(0_i32, |value, digit| {
                value
                    .saturating_mul(10)
                    .saturating_add(i32::from(digit - b'0'))
            });
        highest = highest.max(number);
    }
    highest
}

fn register_limit(disassemble: Option<Disassemble>, code: &[u8]) -> i32 {
    let Some(disassemble) = disassemble else {
        return -1;
    };
    let mut assembly = ptr::null_mut();
    // The input bytes and out pointer stay valid through the call.
    let status = unsafe { disassemble(code.as_ptr(), code.len(), 0, ptr::null(), &mut assembly) };
    OwnedBlob::from_output(assembly)
        .filter(|_| status >= 0)
        .map_or(-1, |blob| highest_temp(blob.bytes()))
}

fn run(args: &[std::ffi::OsString]) -> Result<bool, Box<dyn std::error::Error>> {
    let [source_path, profile, include_dir, extra @ ..] = args else {
        return Err(
            "Usage: hlsl_compile source profile include-directory [all|lights] [mode]".into(),
        );
    };
    let dll =
        env::var_os("GARRYSCAPE_D3DCOMPILER_DLL").unwrap_or_else(|| "d3dcompiler_47.dll".into());
    let library =
        Library::load(&dll).map_err(|err| format!("Cannot load Microsoft compiler DLL: {err}"))?;
    let compile = library.symbol(c"D3DCompile");
    if compile.is_null() {
        return Err("Microsoft compiler DLL does not export D3DCompile".into());
    }
    let disassemble = library.symbol(c"D3DDisassemble");
    // The symbol names determine the Windows ABI signatures. Keep the DLL loaded through all calls.
    let compile = unsafe { std::mem::transmute::<*mut c_void, Compile>(compile) };
    let disassemble = (!disassemble.is_null())
        .then(|| unsafe { std::mem::transmute::<*mut c_void, Disassemble>(disassemble) });
    let source_path = Path::new(source_path);
    let source = fs::read(source_path)
        .map_err(|err| format!("Cannot read {}: {err}", source_path.display()))?;
    let source_name = CString::new(source_path.to_string_lossy().as_bytes())?;
    let profile = profile.to_string_lossy();
    let target = CString::new(profile.as_bytes())?;
    let shader_model = if profile.starts_with('v') {
        c"SHADER_MODEL_VS_3_0"
    } else {
        c"SHADER_MODEL_PS_3_0"
    };
    let mut includes = Includes {
        vtable: &INCLUDE_VTABLE,
        root: PathBuf::from(include_dir),
        buffers: HashMap::new(),
    };
    let pairs = if extra.first().is_some_and(|arg| arg == "all") {
        (0..2)
            .flat_map(|mode| (0..5).map(move |lights| (lights.to_string(), mode.to_string())))
            .collect()
    } else {
        let option = |index| {
            extra.get(index).map_or_else(
                || String::from("0"),
                |arg: &std::ffi::OsString| arg.to_string_lossy().into_owned(),
            )
        };
        vec![(option(0), option(1))]
    };

    let mut stdout = io::stdout().lock();
    let mut failed = false;
    for (lights, mode) in pairs {
        let lights_value = CString::new(lights.as_bytes())?;
        let mode_value = CString::new(mode.as_bytes())?;
        let macros = [
            Macro::new(c"NUM_LIGHTS", &lights_value),
            Macro::new(c"SKINNING", &mode_value),
            Macro::new(c"FLASHLIGHT", &mode_value),
            Macro::new(c"X64", c"1"),
            Macro::new(c"COMPRESSED_VERTS", c"1"),
            Macro::new(shader_model, c"1"),
            Macro::new(c"SCR_W", c"1920"),
            Macro::new(c"SCR_H", c"1080"),
            Macro::new(c"SCR_S", c"float2(SCR_W,SCR_H)"),
            Macro {
                name: ptr::null(),
                value: ptr::null(),
            },
        ];
        let (mut code, mut errors) = (ptr::null_mut(), ptr::null_mut());
        // All pointers remain valid for the synchronous call. Flags match EGSM.
        let status = unsafe {
            compile(
                source.as_ptr(),
                source.len(),
                source_name.as_ptr(),
                macros.as_ptr(),
                &mut includes,
                c"main".as_ptr(),
                target.as_ptr(),
                0,
                0,
                &mut code,
                &mut errors,
            )
        };
        let code = OwnedBlob::from_output(code);
        let errors = OwnedBlob::from_output(errors);
        if let Some(code) = code.filter(|_| status >= 0) {
            let bytes = code.bytes();
            let highest = register_limit(disassemble, bytes);
            writeln!(
                stdout,
                "PASS {profile} lights={lights} mode={mode} bytes={} highest_temp=r{highest}",
                bytes.len()
            )?;
        } else {
            writeln!(stdout, "FAIL {profile} lights={lights} mode={mode}")?;
            if let Some(errors) = errors {
                stdout.write_all(errors.bytes())?;
            }
            failed = true;
        }
        stdout.flush()?;
    }
    Ok(failed)
}

fn main() -> ExitCode {
    match run(&env::args_os().skip(1).collect::<Vec<_>>()) {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(1),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
