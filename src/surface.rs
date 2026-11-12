use std::ffi::c_void;
use std::ptr::null_mut;

use tiny_skia::Pixmap;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

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

    pub fn present(&self, hwnd: HWND, pixmap: &Pixmap) {
        if pixmap.width() != self.width as u32 || pixmap.height() != self.height as u32 {
            return;
        }

        unsafe {
            let data = pixmap.data();
            let target = std::slice::from_raw_parts_mut(
                self.bits,
                (self.width as usize) * (self.height as usize) * 4,
            );
            for (dst, src) in target.chunks_exact_mut(4).zip(data.chunks_exact(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = src[3];
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
            let _ = UpdateLayeredWindow(
                hwnd,
                self.screen,
                None,
                Some(&size),
                self.memory,
                Some(&src_point),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
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
