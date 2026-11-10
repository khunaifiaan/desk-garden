//! GPU side of the garden: layer textures, the full-screen scene shader and the
//! particle pass (leaves, fireflies, rain). Everything is authored in the 1920x1080
//! design space and cover-fitted to the monitor in the shaders.

use crate::gfx::{self, Gpu, Texture};
use windows::core::s;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const BASE_PNG: &[u8] = include_bytes!("../assets/layers/L_base.png");
const BED_PNG: &[u8] = include_bytes!("../assets/layers/L_bed.png");
const OBJ_PNG: &[u8] = include_bytes!("../assets/layers/L_obj.png");
const FERN_PNG: &[u8] = include_bytes!("../assets/layers/L_fern.png");
const CANOPY_PNG: &[u8] = include_bytes!("../assets/layers/L_canopy.png");

pub const DESIGN_W: f32 = 1920.0;
pub const DESIGN_H: f32 = 1080.0;
pub const MAX_RIPPLES: usize = 32;
pub const MAX_PARTICLES: usize = 1024;

/// Particle kinds understood by the particle shader.
pub const P_RING: f32 = 0.0;
pub const P_DISC: f32 = 1.0;
pub const P_STREAK: f32 = 2.0;
pub const P_LEAF: f32 = 3.0;

// Shaders are compiled at build time (build.rs) so d3dcompiler is never loaded at runtime.
const VS_FULL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vs_full.cso"));
const PS_SCENE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ps_scene.cso"));
const VS_PARTICLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vs_particle.cso"));
const PS_PARTICLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ps_particle.cso"));

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FrameCb {
    pub screen: [f32; 2],
    pub scale: f32,
    pub time: f32,
    pub offset: [f32; 2],
    pub sway_fern: f32,
    pub sway_canopy: f32,
    pub grade_mul: [f32; 3],
    pub night: f32,
    pub grade_screen: [f32; 4],
    pub desat: f32,
    pub contrast: f32,
    pub bright: f32,
    pub layer_desat: f32,
    pub weather_mul: [f32; 3],
    pub layer_bright: f32,
    pub basin_level: f32,
    pub spout_phase: f32,
    pub splash: f32,
    pub ripple_count: f32,
    pub fern_rect: [f32; 4],
    pub canopy_rect: [f32; 4],
    pub ripples: [[f32; 4]; MAX_RIPPLES],
}

impl Default for FrameCb {
    fn default() -> Self {
        FrameCb {
            screen: [1.0, 1.0],
            scale: 1.0,
            time: 0.0,
            offset: [0.0, 0.0],
            sway_fern: 0.0,
            sway_canopy: 0.0,
            grade_mul: [1.0; 3],
            night: 0.0,
            grade_screen: [0.0; 4],
            desat: 0.0,
            contrast: 1.0,
            bright: 1.0,
            layer_desat: 0.0,
            weather_mul: [1.0; 3],
            layer_bright: 1.0,
            basin_level: 0.0,
            spout_phase: 0.0,
            splash: 0.0,
            ripple_count: 0.0,
            fern_rect: [0.0, 0.0, 1920.0, 1080.0],
            canopy_rect: [0.0, 0.0, 1920.0, 1080.0],
            ripples: [[0.0; 4]; MAX_RIPPLES],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Particle {
    pub pos: [f32; 2],
    pub ext: [f32; 2],
    pub col: [f32; 4],
    /// kind, quad rotation, stroke width (ring/streak) or radius (leaf), graded flag
    pub prm: [f32; 4],
    /// ring/disc: rx, ry ; leaf: rotation, squash
    pub shp: [f32; 4],
}

pub struct Scene {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    pvs: ID3D11VertexShader,
    pps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    instances: ID3D11Buffer,
    blend: ID3D11BlendState,
    cb: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    border: ID3D11SamplerState,
    textures: [Texture; 4],
    fern_rect: [f32; 4],
    canopy_rect: [f32; 4],
}

/// Crop an RGBA layer to the bounding box of its visible pixels, plus a margin so the
/// blurred shadow mips fade out inside the texture. Returns (rect in design px, w, h, pixels).
fn crop_rgba(w: u32, h: u32, px: &[u8], margin: u32) -> ([f32; 4], u32, u32, Vec<u8>) {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if px[((y * w + x) * 4 + 3) as usize] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 {
        return ([0.0, 0.0, 1.0, 1.0], 1, 1, vec![0; 4]);
    }
    // The margin may extend past the design edges (transparent there).
    let (cx0, cy0) = (x0 as i32 - margin as i32, y0 as i32 - margin as i32);
    let (cw, ch) = (x1 - x0 + 2 * margin, y1 - y0 + 2 * margin);
    let mut out = vec![0u8; (cw * ch * 4) as usize];
    for y in 0..ch {
        let sy = cy0 + y as i32;
        if sy < 0 || sy >= h as i32 {
            continue;
        }
        for x in 0..cw {
            let sx = cx0 + x as i32;
            if sx < 0 || sx >= w as i32 {
                continue;
            }
            let si = ((sy as u32 * w + sx as u32) * 4) as usize;
            let di = ((y * cw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&px[si..si + 4]);
        }
    }
    ([cx0 as f32, cy0 as f32, cw as f32, ch as f32], cw, ch, out)
}

/// Cover-fit transform: (scale, offset_x, offset_y) mapping design px to screen px.
pub fn cover_fit(w: f32, h: f32) -> (f32, f32, f32) {
    let s = (w / DESIGN_W).max(h / DESIGN_H);
    (s, (w - DESIGN_W * s) * 0.5, (h - DESIGN_H * s) * 0.5)
}

/// CPU copy of the rake mask at 1/4 resolution, used only to decide where the cursor spawns ripples.
pub struct RakeMaskCpu {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl RakeMaskCpu {
    pub fn at(&self, dx: f32, dy: f32) -> f32 {
        let x = (dx / DESIGN_W * self.w as f32) as i32;
        let y = (dy / DESIGN_H * self.h as f32) as i32;
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return 0.0;
        }
        self.data[(y as u32 * self.w + x as u32) as usize] as f32 / 255.0
    }
}

fn gray(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let (w, h, ch, px) = gfx::decode_png(bytes);
    let v = px.chunks_exact(ch as usize).map(|p| p[0]).collect();
    (w, h, v)
}

impl Scene {
    pub fn new(gpu: &Gpu) -> windows::core::Result<(Scene, RakeMaskCpu)> {
        unsafe {
            let dev = &gpu.device;
            let mut vs = None;
            dev.CreateVertexShader(VS_FULL, None, Some(&mut vs))?;
            let mut ps = None;
            dev.CreatePixelShader(PS_SCENE, None, Some(&mut ps))?;
            let mut pps = None;
            dev.CreatePixelShader(PS_PARTICLE, None, Some(&mut pps))?;
            let mut pvs = None;
            dev.CreateVertexShader(VS_PARTICLE, None, Some(&mut pvs))?;

            let el = |name: windows::core::PCSTR, fmt: DXGI_FORMAT, off: u32| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: name,
                SemanticIndex: 0,
                Format: fmt,
                InputSlot: 0,
                AlignedByteOffset: off,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            };
            let elems = [
                el(s!("POS"), DXGI_FORMAT_R32G32_FLOAT, 0),
                el(s!("EXT"), DXGI_FORMAT_R32G32_FLOAT, 8),
                el(s!("COL"), DXGI_FORMAT_R32G32B32A32_FLOAT, 16),
                el(s!("PRM"), DXGI_FORMAT_R32G32B32A32_FLOAT, 32),
                el(s!("SHP"), DXGI_FORMAT_R32G32B32A32_FLOAT, 48),
            ];
            let mut layout = None;
            dev.CreateInputLayout(&elems, VS_PARTICLE, Some(&mut layout))?;

            let desc = D3D11_BUFFER_DESC {
                ByteWidth: (std::mem::size_of::<Particle>() * MAX_PARTICLES) as u32,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                ..Default::default()
            };
            let mut instances = None;
            dev.CreateBuffer(&desc, None, Some(&mut instances))?;

            let mut bd = D3D11_BLEND_DESC::default();
            bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            let mut blend = None;
            dev.CreateBlendState(&bd, Some(&mut blend))?;

            // Layer textures. Each decoded buffer is dropped right after upload.
            let base = {
                let (w, h, ch, px) = gfx::decode_png(BASE_PNG);
                let rgba = if ch == 3 { gfx::rgb_to_rgba(&px) } else { px };
                gpu.texture(w, h, DXGI_FORMAT_R8G8B8A8_UNORM, 4, &rgba, false)?
            };
            let (mask_tex, cpu_mask) = {
                let (w, h, bed) = gray(BED_PNG);
                let (_, _, obj) = gray(OBJ_PNG);
                // Half resolution is plenty for a soft mask and saves 1.5 MB.
                let (hw, hh) = (w / 2, h / 2);
                let mut m = vec![0u8; (hw * hh) as usize];
                for y in 0..hh {
                    for x in 0..hw {
                        let mut acc = 0u32;
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let i = ((2 * y + dy) * w + 2 * x + dx) as usize;
                            acc += bed[i] as u32 * (255 - obj[i] as u32) / 255;
                        }
                        m[(y * hw + x) as usize] = (acc / 4) as u8;
                    }
                }
                drop((bed, obj));
                let (qw, qh) = (hw / 2, hh / 2);
                let mut q = vec![0u8; (qw * qh) as usize];
                for y in 0..qh {
                    for x in 0..qw {
                        q[(y * qw + x) as usize] = m[((y * 2 + 1) * hw + x * 2 + 1) as usize];
                    }
                }
                (gpu.texture(hw, hh, DXGI_FORMAT_R8_UNORM, 1, &m, false)?, RakeMaskCpu { w: qw, h: qh, data: q })
            };
            let rgba_mips = |bytes: &[u8]| -> windows::core::Result<(Texture, [f32; 4])> {
                let (w, h, ch, px) = gfx::decode_png(bytes);
                let rgba = if ch == 3 { gfx::rgb_to_rgba(&px) } else { px };
                let (rect, cw, chh, cropped) = crop_rgba(w, h, &rgba, 48);
                drop(rgba);
                Ok((gpu.texture(cw, chh, DXGI_FORMAT_R8G8B8A8_UNORM, 4, &cropped, true)?, rect))
            };
            let (fern, fern_rect) = rgba_mips(FERN_PNG)?;
            let (canopy, canopy_rect) = rgba_mips(CANOPY_PNG)?;

            Ok((
                Scene {
                    vs: vs.unwrap(),
                    ps: ps.unwrap(),
                    pvs: pvs.unwrap(),
                    pps: pps.unwrap(),
                    layout: layout.unwrap(),
                    instances: instances.unwrap(),
                    blend: blend.unwrap(),
                    cb: gpu.constant_buffer(std::mem::size_of::<FrameCb>() as u32)?,
                    sampler: gfx::sampler(dev, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_CLAMP)?,
                    border: gfx::sampler(dev, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_BORDER)?,
                    textures: [base, mask_tex, fern, canopy],
                    fern_rect,
                    canopy_rect,
                },
                cpu_mask,
            ))
        }
    }

    pub fn draw(&self, gpu: &Gpu, mut cb: FrameCb, particles: &[Particle]) {
        let (scale, ox, oy) = cover_fit(gpu.width as f32, gpu.height as f32);
        cb.screen = [gpu.width as f32, gpu.height as f32];
        cb.scale = scale;
        cb.offset = [ox, oy];
        cb.fern_rect = self.fern_rect;
        cb.canopy_rect = self.canopy_rect;
        gpu.write_buffer(&self.cb, &[cb]);
        let n = particles.len().min(MAX_PARTICLES);
        if n > 0 {
            gpu.write_buffer(&self.instances, &particles[..n]);
        }
        unsafe {
            let c = &gpu.ctx;
            gpu.begin();
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.IASetInputLayout(None);
            c.VSSetShader(&self.vs, None);
            c.PSSetShader(&self.ps, None);
            c.VSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            c.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            let srvs: Vec<Option<ID3D11ShaderResourceView>> = self.textures.iter().map(|t| Some(t.srv.clone())).collect();
            c.PSSetShaderResources(0, Some(&srvs));
            c.PSSetSamplers(0, Some(&[Some(self.sampler.clone()), Some(self.border.clone())]));
            c.OMSetBlendState(None, None, 0xffff_ffff);
            c.Draw(3, 0);

            if n > 0 {
                c.IASetInputLayout(&self.layout);
                let stride = std::mem::size_of::<Particle>() as u32;
                let off = 0u32;
                c.IASetVertexBuffers(0, 1, Some(&Some(self.instances.clone())), Some(&stride), Some(&off));
                c.VSSetShader(&self.pvs, None);
                c.PSSetShader(&self.pps, None);
                c.OMSetBlendState(&self.blend, None, 0xffff_ffff);
                c.DrawInstanced(6, n as u32, 0, 0);
            }
        }
    }
}
