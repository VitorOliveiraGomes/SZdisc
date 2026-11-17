use std::mem::zeroed;
use std::ptr::null_mut;

use winapi::shared::dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGIResource,
};
use winapi::shared::dxgi1_2::{IDXGIOutput1, IDXGIOutputDuplication, DXGI_OUTDUPL_FRAME_INFO};
use winapi::shared::dxgiformat::DXGI_FORMAT_B8G8R8A8_UNORM;
use winapi::shared::dxgitype::DXGI_SAMPLE_DESC;
use winapi::shared::minwindef::UINT;
use winapi::shared::winerror::{DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, SUCCEEDED};
use winapi::um::d3d11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use winapi::um::d3dcommon::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN};
use winapi::um::unknwnbase::IUnknown;
use winapi::Interface;

pub(crate) enum DxgiCaptureResult {
    NewFrame,
    CachedFrame,
    Unchanged,
    Unavailable,
}

pub(crate) struct DxgiDuplicator {
    device: *mut ID3D11Device,
    context: *mut ID3D11DeviceContext,
    dupl: *mut IDXGIOutputDuplication,
    staging: *mut ID3D11Texture2D,
    staging_w: u32,
    staging_h: u32,
    out_left: i32,
    out_top: i32,
    out_w: i32,
    out_h: i32,
    last_bgra: Vec<u8>,
    last_left: i32,
    last_top: i32,
    last_w: i32,
    last_h: i32,
}

impl DxgiDuplicator {
    pub(crate) unsafe fn release_all(&mut self) {
        if !self.staging.is_null() {
            (*self.staging).Release();
            self.staging = null_mut();
        }
        if !self.dupl.is_null() {
            (*self.dupl).Release();
            self.dupl = null_mut();
        }
        if !self.context.is_null() {
            (*self.context).Release();
            self.context = null_mut();
        }
        if !self.device.is_null() {
            (*self.device).Release();
            self.device = null_mut();
        }
        self.staging_w = 0;
        self.staging_h = 0;
        self.last_bgra.clear();
        self.last_w = 0;
        self.last_h = 0;
    }

    pub(crate) fn contains_rect(
        &self,
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
    ) -> bool {
        src_left >= self.out_left
            && src_top >= self.out_top
            && src_left + src_w <= self.out_left + self.out_w
            && src_top + src_h <= self.out_top + self.out_h
    }

    /// Enumera todos os adaptadores DXGI (`IDXGIFactory1`) para suportar notebooks híbridos.
    pub(crate) unsafe fn init_for_region(
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
        fallback_idx: usize,
    ) -> Option<Self> {
        let mut factory: *mut IDXGIFactory1 = null_mut();
        let hr_fac = CreateDXGIFactory1(&IDXGIFactory1::uuidof(), &mut factory as *mut _ as *mut _);
        if !SUCCEEDED(hr_fac) || factory.is_null() {
            return Self::init_default_adapter(src_left, src_top, src_w, src_h, fallback_idx);
        }

        let mut chosen_pair: Option<(*mut IDXGIAdapter1, *mut IDXGIOutput)> = None;
        let mut fallback_pair: Option<(*mut IDXGIAdapter1, *mut IDXGIOutput)> = None;
        let mut global_out_idx: usize = 0;
        let mut adapter_idx: UINT = 0;

        loop {
            let mut adapter: *mut IDXGIAdapter1 = null_mut();
            if !SUCCEEDED((*factory).EnumAdapters1(adapter_idx, &mut adapter)) || adapter.is_null()
            {
                break;
            }

            let mut out_idx: UINT = 0;
            loop {
                let mut cand: *mut IDXGIOutput = null_mut();
                if !SUCCEEDED((*adapter).EnumOutputs(out_idx, &mut cand)) || cand.is_null() {
                    break;
                }

                let mut desc = zeroed();
                let _ = (*cand).GetDesc(&mut desc);
                let r = desc.DesktopCoordinates;
                let matches = src_left >= r.left
                    && src_top >= r.top
                    && src_left + src_w <= r.right
                    && src_top + src_h <= r.bottom;

                if matches && chosen_pair.is_none() {
                    (*adapter).AddRef();
                    chosen_pair = Some((adapter, cand));
                    break;
                } else if global_out_idx == fallback_idx && fallback_pair.is_none() {
                    (*adapter).AddRef();
                    fallback_pair = Some((adapter, cand));
                } else {
                    (*cand).Release();
                }

                global_out_idx += 1;
                out_idx += 1;
            }

            (*adapter).Release();
            if chosen_pair.is_some() {
                break;
            }
            adapter_idx += 1;
        }
        (*factory).Release();

        let (adapter, output) = match chosen_pair {
            Some(pair) => {
                if let Some((fb_ad, fb_out)) = fallback_pair {
                    (*fb_out).Release();
                    (*fb_ad).Release();
                }
                pair
            }
            None => fallback_pair?,
        };

        let mut device: *mut ID3D11Device = null_mut();
        let mut context: *mut ID3D11DeviceContext = null_mut();
        let hr = D3D11CreateDevice(
            adapter as *mut IDXGIAdapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            null_mut(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            null_mut(),
            0,
            D3D11_SDK_VERSION,
            &mut device,
            null_mut(),
            &mut context,
        );
        (*adapter).Release();

        if !SUCCEEDED(hr) || device.is_null() || context.is_null() {
            (*output).Release();
            if !context.is_null() {
                (*context).Release();
            }
            if !device.is_null() {
                (*device).Release();
            }
            return None;
        }

        Self::finish_init_with_output(device, context, output)
    }

    unsafe fn init_default_adapter(
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
        fallback_idx: usize,
    ) -> Option<Self> {
        let mut device: *mut ID3D11Device = null_mut();
        let mut context: *mut ID3D11DeviceContext = null_mut();
        let hr = D3D11CreateDevice(
            null_mut(),
            D3D_DRIVER_TYPE_HARDWARE,
            null_mut(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            null_mut(),
            0,
            D3D11_SDK_VERSION,
            &mut device,
            null_mut(),
            &mut context,
        );
        if !SUCCEEDED(hr) || device.is_null() || context.is_null() {
            return None;
        }

        let mut dxgi_dev: *mut winapi::shared::dxgi::IDXGIDevice = null_mut();
        if !SUCCEEDED((*device).QueryInterface(
            &winapi::shared::dxgi::IDXGIDevice::uuidof(),
            &mut dxgi_dev as *mut _ as *mut _,
        )) || dxgi_dev.is_null()
        {
            (*context).Release();
            (*device).Release();
            return None;
        }

        let mut adapter = null_mut();
        let hr_ad = (*dxgi_dev).GetAdapter(&mut adapter);
        (*dxgi_dev).Release();
        if !SUCCEEDED(hr_ad) || adapter.is_null() {
            (*context).Release();
            (*device).Release();
            return None;
        }

        let mut chosen: *mut IDXGIOutput = null_mut();
        let mut fallback: *mut IDXGIOutput = null_mut();
        let mut i: UINT = 0;
        loop {
            let mut cand: *mut IDXGIOutput = null_mut();
            if !SUCCEEDED((*adapter).EnumOutputs(i, &mut cand)) || cand.is_null() {
                break;
            }
            let mut desc = zeroed();
            let _ = (*cand).GetDesc(&mut desc);
            let r = desc.DesktopCoordinates;
            if src_left >= r.left
                && src_top >= r.top
                && src_left + src_w <= r.right
                && src_top + src_h <= r.bottom
            {
                chosen = cand;
                break;
            } else if i as usize == fallback_idx && fallback.is_null() {
                fallback = cand;
            } else {
                (*cand).Release();
            }
            i += 1;
        }
        (*adapter).Release();

        let output = if !chosen.is_null() {
            if !fallback.is_null() {
                (*fallback).Release();
            }
            chosen
        } else {
            fallback
        };

        if output.is_null() {
            (*context).Release();
            (*device).Release();
            return None;
        }

        Self::finish_init_with_output(device, context, output)
    }

    unsafe fn finish_init_with_output(
        device: *mut ID3D11Device,
        context: *mut ID3D11DeviceContext,
        output: *mut IDXGIOutput,
    ) -> Option<Self> {
        let mut desc = zeroed();
        let _ = (*output).GetDesc(&mut desc);
        let out_left = desc.DesktopCoordinates.left;
        let out_top = desc.DesktopCoordinates.top;
        let out_w = desc.DesktopCoordinates.right - desc.DesktopCoordinates.left;
        let out_h = desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top;

        let mut output1: *mut IDXGIOutput1 = null_mut();
        let hr_o1 =
            (*output).QueryInterface(&IDXGIOutput1::uuidof(), &mut output1 as *mut _ as *mut _);
        (*output).Release();
        if !SUCCEEDED(hr_o1) || output1.is_null() {
            (*context).Release();
            (*device).Release();
            return None;
        }

        let mut dupl: *mut IDXGIOutputDuplication = null_mut();
        let hr_dup = (*output1).DuplicateOutput(device as *mut IUnknown, &mut dupl);
        (*output1).Release();
        if !SUCCEEDED(hr_dup) || dupl.is_null() {
            (*context).Release();
            (*device).Release();
            return None;
        }

        Some(Self {
            device,
            context,
            dupl,
            staging: null_mut(),
            staging_w: 0,
            staging_h: 0,
            out_left,
            out_top,
            out_w,
            out_h,
            last_bgra: Vec::new(),
            last_left: 0,
            last_top: 0,
            last_w: 0,
            last_h: 0,
        })
    }

    unsafe fn ensure_staging(&mut self, w: u32, h: u32) -> bool {
        if !self.staging.is_null() && self.staging_w == w && self.staging_h == h {
            return true;
        }
        if !self.staging.is_null() {
            (*self.staging).Release();
            self.staging = null_mut();
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ,
            MiscFlags: 0,
        };
        let hr = (*self.device).CreateTexture2D(&desc, null_mut(), &mut self.staging);
        if SUCCEEDED(hr) && !self.staging.is_null() {
            self.staging_w = w;
            self.staging_h = h;
            true
        } else {
            self.staging = null_mut();
            self.staging_w = 0;
            self.staging_h = 0;
            false
        }
    }

    pub(crate) unsafe fn capture_crop_bgra(
        &mut self,
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
        allow_cached: bool,
        out_bgra: &mut Vec<u8>,
    ) -> DxgiCaptureResult {
        if !self.contains_rect(src_left, src_top, src_w, src_h) {
            return DxgiCaptureResult::Unavailable;
        }

        let mut frame_info: DXGI_OUTDUPL_FRAME_INFO = zeroed();
        let mut resource: *mut IDXGIResource = null_mut();
        let hr = (*self.dupl).AcquireNextFrame(8, &mut frame_info, &mut resource);
        if hr == DXGI_ERROR_WAIT_TIMEOUT {
            return self.try_cached_frame(src_left, src_top, src_w, src_h, allow_cached, out_bgra);
        }
        if hr == DXGI_ERROR_ACCESS_LOST || !SUCCEEDED(hr) || resource.is_null() {
            if !resource.is_null() {
                (*resource).Release();
            }
            return DxgiCaptureResult::Unavailable;
        }

        if frame_info.AccumulatedFrames == 0 && frame_info.TotalMetadataBufferSize == 0 {
            (*resource).Release();
            let _ = (*self.dupl).ReleaseFrame();
            return self.try_cached_frame(src_left, src_top, src_w, src_h, allow_cached, out_bgra);
        }

        let mut tex: *mut ID3D11Texture2D = null_mut();
        let hr_tex =
            (*resource).QueryInterface(&ID3D11Texture2D::uuidof(), &mut tex as *mut _ as *mut _);
        (*resource).Release();
        if !SUCCEEDED(hr_tex) || tex.is_null() {
            let _ = (*self.dupl).ReleaseFrame();
            return DxgiCaptureResult::Unavailable;
        }

        let uw = src_w as u32;
        let uh = src_h as u32;
        if !self.ensure_staging(uw, uh) {
            (*tex).Release();
            let _ = (*self.dupl).ReleaseFrame();
            return DxgiCaptureResult::Unavailable;
        }

        let rel_left = (src_left - self.out_left) as u32;
        let rel_top = (src_top - self.out_top) as u32;
        let src_box = D3D11_BOX {
            left: rel_left,
            top: rel_top,
            front: 0,
            right: rel_left + uw,
            bottom: rel_top + uh,
            back: 1,
        };
        (*self.context).CopySubresourceRegion(
            self.staging as *mut ID3D11Resource,
            0,
            0,
            0,
            0,
            tex as *mut ID3D11Resource,
            0,
            &src_box,
        );
        (*tex).Release();
        let _ = (*self.dupl).ReleaseFrame();

        let mut mapped: D3D11_MAPPED_SUBRESOURCE = zeroed();
        let hr_map = (*self.context).Map(
            self.staging as *mut ID3D11Resource,
            0,
            D3D11_MAP_READ,
            0,
            &mut mapped,
        );
        if !SUCCEEDED(hr_map) || mapped.pData.is_null() {
            return DxgiCaptureResult::Unavailable;
        }

        let row_bytes = (uw as usize) * 4;
        let total_bytes = row_bytes * (uh as usize);
        self.last_bgra.resize(total_bytes, 0);
        let src_ptr = mapped.pData as *const u8;
        let pitch = mapped.RowPitch as usize;
        if pitch == row_bytes {
            std::ptr::copy_nonoverlapping(src_ptr, self.last_bgra.as_mut_ptr(), total_bytes);
        } else {
            for y in 0..(uh as usize) {
                std::ptr::copy_nonoverlapping(
                    src_ptr.add(y * pitch),
                    self.last_bgra.as_mut_ptr().add(y * row_bytes),
                    row_bytes,
                );
            }
        }
        (*self.context).Unmap(self.staging as *mut ID3D11Resource, 0);

        self.last_left = src_left;
        self.last_top = src_top;
        self.last_w = src_w;
        self.last_h = src_h;

        out_bgra.resize(total_bytes, 0);
        out_bgra.copy_from_slice(&self.last_bgra);
        DxgiCaptureResult::NewFrame
    }

    fn try_cached_frame(
        &self,
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
        allow_cached: bool,
        out_bgra: &mut Vec<u8>,
    ) -> DxgiCaptureResult {
        if allow_cached
            && !self.last_bgra.is_empty()
            && self.last_left == src_left
            && self.last_top == src_top
            && self.last_w == src_w
            && self.last_h == src_h
        {
            out_bgra.resize(self.last_bgra.len(), 0);
            out_bgra.copy_from_slice(&self.last_bgra);
            DxgiCaptureResult::CachedFrame
        } else {
            DxgiCaptureResult::Unchanged
        }
    }
}
