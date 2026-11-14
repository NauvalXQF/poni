use std::ffi::c_void;
use std::ptr::null_mut;

use tiny_skia::Pixmap;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    UpdateLayeredWindowIndirect, ULW_ALPHA, UPDATELAYEREDWINDOWINFO,
};

pub struct LayeredSurface {
    pub(crate) width: i32,
    pub(crate) height: i32,
    screen: HDC,
    memory: HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u8,
}

impl LayeredSurface {
    pub fn new(width: i32, height: i32) -> windows::core::Result<Self> {
        if width <= 0 || height <= 0 {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }

        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err(windows::core::Error::from_win32());
            }
            let memory = CreateCompatibleDC(screen);
            if memory.is_invalid() {
                let _ = ReleaseDC(None, screen);
                return Err(windows::core::Error::from_win32());
            }

            let bitmap_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = null_mut();
            let bitmap =
                match CreateDIBSection(screen, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) {
                    Ok(bitmap) => bitmap,
                    Err(err) => {
                        let _ = DeleteDC(memory);
                        let _ = ReleaseDC(None, screen);
                        return Err(err);
                    }
                };
            let old_bitmap = SelectObject(memory, HGDIOBJ(bitmap.0));

            Ok(Self {
                width,
                height,
                screen,
                memory,
                bitmap,
                old_bitmap,
                bits: bits.cast(),
            })
        }
    }

    pub fn present(&self, hwnd: HWND, pixmap: &Pixmap, dirty: Option<(i32, i32, i32, i32)>) {
        if pixmap.width() != self.width as u32 || pixmap.height() != self.height as u32 {
            return;
        }

        let (x0, y0, x1, y1) = match dirty {
            Some(rect) => (
                rect.0.clamp(0, self.width),
                rect.1.clamp(0, self.height),
                rect.2.clamp(0, self.width),
                rect.3.clamp(0, self.height),
            ),
            None => (0, 0, self.width, self.height),
        };
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        unsafe {
            let src = pixmap.data();
            let dst = std::slice::from_raw_parts_mut(
                self.bits,
                (self.width as usize) * (self.height as usize) * 4,
            );
            let row_bytes = self.width as usize * 4;
            for y in y0 as usize..y1 as usize {
                let start = y * row_bytes + x0 as usize * 4;
                let end = y * row_bytes + x1 as usize * 4;
                for (dst_px, src_px) in dst[start..end]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(src[start..end].as_chunks::<4>().0)
                {
                    dst_px[0] = src_px[2];
                    dst_px[1] = src_px[1];
                    dst_px[2] = src_px[0];
                    dst_px[3] = src_px[3];
                }
            }

            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let src_point = POINT { x: 0, y: 0 };
            let dirty_rect = windows::Win32::Foundation::RECT {
                left: x0,
                top: y0,
                right: x1,
                bottom: y1,
            };
            let info = UPDATELAYEREDWINDOWINFO {
                cbSize: std::mem::size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
                hdcDst: self.screen,
                pptDst: null_mut(),
                psize: &size,
                hdcSrc: self.memory,
                pptSrc: &src_point,
                crKey: COLORREF(0),
                pblend: &blend,
                dwFlags: ULW_ALPHA,
                // Only the capsule region moves; leaving the rest of the
                // layered surface alone keeps per-frame uploads small.
                prcDirty: if dirty.is_some() {
                    &dirty_rect
                } else {
                    null_mut()
                },
            };
            let _ = UpdateLayeredWindowIndirect(hwnd, &info);
        }
    }
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.memory, self.old_bitmap);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.memory);
            let _ = ReleaseDC(None, self.screen);
        }
    }
}
