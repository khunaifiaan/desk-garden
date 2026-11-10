//! Compiles src/garden.hlsl to bytecode at build time, so the app never loads
//! d3dcompiler at runtime (it costs tens of MB of heap and ~0.5 s of startup).

use std::ffi::c_void;
use windows::core::{s, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::ID3DBlob;

fn bytes(b: &ID3DBlob) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec() }
}

fn main() {
    println!("cargo:rerun-if-changed=src/garden.hlsl");
    let src = std::fs::read_to_string("src/garden.hlsl").expect("read src/garden.hlsl");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let shaders: [(&str, PCSTR, PCSTR); 4] = [
        ("vs_full", s!("vs_full"), s!("vs_4_0")),
        ("ps_scene", s!("ps_scene"), s!("ps_4_0")),
        ("vs_particle", s!("vs_particle"), s!("vs_4_0")),
        ("ps_particle", s!("ps_particle"), s!("ps_4_0")),
    ];
    for (name, entry, target) in shaders {
        let mut code: Option<ID3DBlob> = None;
        let mut err: Option<ID3DBlob> = None;
        let r = unsafe {
            D3DCompile(src.as_ptr() as *const c_void, src.len(), s!("garden.hlsl"), None, None, entry, target, D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, &mut code, Some(&mut err))
        };
        if r.is_err() {
            let msg = err.map(|e| String::from_utf8_lossy(&bytes(&e)).into_owned()).unwrap_or_default();
            panic!("shader {name} failed to compile:\n{msg}");
        }
        std::fs::write(out.join(format!("{name}.cso")), bytes(&code.unwrap())).unwrap();
    }
}
