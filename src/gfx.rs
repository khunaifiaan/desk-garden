//! Direct3D 11 renderer. Layer images are decoded once, uploaded to the GPU and the
//! CPU copies dropped. All per-pixel work happens in shaders.

use std::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

pub struct Texture {
    pub srv: ID3D11ShaderResourceView,
}

pub struct Gpu {
    pub device: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
    swap: Option<IDXGISwapChain1>,
    rtv: Option<ID3D11RenderTargetView>,
    pub width: u32,
    pub height: u32,
}

impl Gpu {
    pub fn new() -> windows::core::Result<Gpu> {
        unsafe {
            let mut device = None;
            let mut ctx = None;
            let levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )?;
            Ok(Gpu { device: device.unwrap(), ctx: ctx.unwrap(), swap: None, rtv: None, width: 0, height: 0 })
        }
    }

    /// (Re)bind the swap chain to a window. Called again after Explorer restarts.
    pub fn bind_window(&mut self, hwnd: HWND, width: u32, height: u32, flip: bool) -> windows::core::Result<()> {
        unsafe {
            self.rtv = None;
            self.swap = None;
            self.ctx.ClearState();
            self.ctx.Flush();
            let dxgi_dev: IDXGIDevice = self.device.cast()?;
            let adapter = dxgi_dev.GetAdapter()?;
            let factory: IDXGIFactory2 = adapter.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: if flip { DXGI_SWAP_EFFECT_FLIP_DISCARD } else { DXGI_SWAP_EFFECT_DISCARD },
                AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
                Flags: 0,
                ..Default::default()
            };
            let swap = factory.CreateSwapChainForHwnd(&self.device, hwnd, &desc, None, None)?;
            let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES);
            let back: ID3D11Texture2D = swap.GetBuffer(0)?;
            let mut rtv = None;
            self.device.CreateRenderTargetView(&back, None, Some(&mut rtv))?;
            self.rtv = rtv;
            self.swap = Some(swap);
            self.width = width;
            self.height = height;
            Ok(())
        }
    }

    pub fn begin(&self) {
        unsafe {
            if let Some(rtv) = &self.rtv {
                self.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
                let vp = D3D11_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: self.width as f32,
                    Height: self.height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                };
                self.ctx.RSSetViewports(Some(&[vp]));
            }
        }
    }

    pub fn present(&self) -> bool {
        unsafe {
            match &self.swap {
                Some(s) => s.Present(1, DXGI_PRESENT(0)).is_ok(),
                None => false,
            }
        }
    }

    /// Drop window-bound resources (window is gone, e.g. Explorer restarted).
    pub fn unbind(&mut self) {
        unsafe {
            self.rtv = None;
            self.swap = None;
            self.ctx.ClearState();
            self.ctx.Flush();
        }
    }

    /// Upload pixels as an immutable texture. With `mips`, a box-filtered mip chain is
    /// built on the CPU (used for the soft overlay shadows) so the driver needs no staging copy.
    pub fn texture(&self, w: u32, h: u32, format: DXGI_FORMAT, bpp: u32, data: &[u8], mips: bool) -> windows::core::Result<Texture> {
        let mut levels: Vec<(u32, u32, Vec<u8>)> = Vec::new();
        if mips {
            let (mut lw, mut lh) = (w, h);
            while lw > 1 || lh > 1 {
                let src: &[u8] = levels.last().map(|l| l.2.as_slice()).unwrap_or(data);
                let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
                let mut dst = vec![0u8; (nw * nh * bpp) as usize];
                for y in 0..nh {
                    for x in 0..nw {
                        for c in 0..bpp {
                            let px = |xx: u32, yy: u32| src[((yy.min(lh - 1) * lw + xx.min(lw - 1)) * bpp + c) as usize] as u32;
                            let v = px(2 * x, 2 * y) + px(2 * x + 1, 2 * y) + px(2 * x, 2 * y + 1) + px(2 * x + 1, 2 * y + 1);
                            dst[((y * nw + x) * bpp + c) as usize] = ((v + 2) / 4) as u8;
                        }
                    }
                }
                levels.push((nw, nh, dst));
                lw = nw;
                lh = nh;
            }
        }
        unsafe {
            let mut init = vec![D3D11_SUBRESOURCE_DATA { pSysMem: data.as_ptr() as *const c_void, SysMemPitch: w * bpp, SysMemSlicePitch: 0 }];
            for (lw, _, d) in &levels {
                init.push(D3D11_SUBRESOURCE_DATA { pSysMem: d.as_ptr() as *const c_void, SysMemPitch: lw * bpp, SysMemSlicePitch: 0 });
            }
            let desc = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: init.len() as u32,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut tex = None;
            self.device.CreateTexture2D(&desc, Some(init.as_ptr()), Some(&mut tex))?;
            let tex: ID3D11Texture2D = tex.unwrap();
            let mut srv = None;
            self.device.CreateShaderResourceView(&tex, None, Some(&mut srv))?;
            Ok(Texture { srv: srv.unwrap() })
        }
    }

    pub fn constant_buffer(&self, size: u32) -> windows::core::Result<ID3D11Buffer> {
        unsafe {
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: (size + 15) & !15,
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                MiscFlags: 0,
                StructureByteStride: 0,
            };
            let mut b = None;
            self.device.CreateBuffer(&desc, None, Some(&mut b))?;
            Ok(b.unwrap())
        }
    }

    pub fn write_buffer<T: Copy>(&self, buf: &ID3D11Buffer, data: &[T]) {
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            if self.ctx.Map(buf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).is_ok() {
                std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, m.pData as *mut u8, std::mem::size_of_val(data));
                self.ctx.Unmap(buf, 0);
            }
        }
    }
}

/// Decode an embedded PNG to tightly packed pixels. Returns (w, h, channels, bytes).
pub fn decode_png(bytes: &[u8]) -> (u32, u32, u32, Vec<u8>) {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().expect("png header");
    let mut buf = vec![0; r.output_buffer_size().expect("png size")];
    let info = r.next_frame(&mut buf).expect("png frame");
    buf.truncate(info.buffer_size());
    let ch = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        _ => 4,
    };
    (info.width, info.height, ch, buf)
}

/// RGB -> RGBA (D3D has no 24-bit format).
pub fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for p in rgb.chunks_exact(3) {
        out.extend_from_slice(&[p[0], p[1], p[2], 255]);
    }
    out
}

pub fn sampler(dev: &ID3D11Device, filter: D3D11_FILTER, addr: D3D11_TEXTURE_ADDRESS_MODE) -> windows::core::Result<ID3D11SamplerState> {
    unsafe {
        let desc = D3D11_SAMPLER_DESC {
            Filter: filter,
            AddressU: addr,
            AddressV: addr,
            AddressW: addr,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: f32::MAX,
        };
        let mut s = None;
        dev.CreateSamplerState(&desc, Some(&mut s))?;
        Ok(s.unwrap())
    }
}
