//! Linear framebuffer driver (GOP-discovered, ours after ExitBootServices).
//! Just enough 2D for the boot banner; the real compositor is a Phase 3
//! milestone. Writes are volatile — the framebuffer is device memory.

#[derive(Clone, Copy)]
pub enum PixelLayout {
    /// Byte order R, G, B, X
    Rgbx,
    /// Byte order B, G, R, X (OVMF's default)
    Bgrx,
}

#[derive(Clone, Copy)]
pub struct Framebuffer {
    base: usize,
    pub width: usize,
    pub height: usize,
    /// Pixels per scanline (>= width on some hardware).
    stride: usize,
    layout: PixelLayout,
}

/// A 1-bit-per-pixel image (row-major, MSB-first within each byte).
pub struct Bitmap1Bpp {
    pub width: usize,
    pub height: usize,
    pub data: &'static [u8],
}

impl Framebuffer {
    pub(crate) fn new(
        base: usize,
        width: usize,
        height: usize,
        stride: usize,
        layout: PixelLayout,
    ) -> Self {
        Self { base, width, height, stride, layout }
    }

    fn encode(&self, r: u8, g: u8, b: u8) -> u32 {
        match self.layout {
            PixelLayout::Rgbx => (r as u32) | (g as u32) << 8 | (b as u32) << 16,
            PixelLayout::Bgrx => (b as u32) | (g as u32) << 8 | (r as u32) << 16,
        }
    }

    #[inline]
    fn put(&self, x: usize, y: usize, pixel: u32) {
        // Hard bounds check even in release: the framebuffer is the one
        // memory region where a stray write is user-visible garbage.
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = y * self.stride + x;
        unsafe {
            (self.base as *mut u32).add(offset).write_volatile(pixel);
        }
    }

    pub fn clear(&self, r: u8, g: u8, b: u8) {
        let pixel = self.encode(r, g, b);
        for y in 0..self.height {
            for x in 0..self.width {
                self.put(x, y, pixel);
            }
        }
    }

    /// Blit a 1-bpp bitmap scaled by an integer factor, centered horizontally
    /// at the given vertical position. Set bits get `fg`; clear bits are left
    /// untouched (transparent background).
    pub fn blit_centered(&self, bitmap: &Bitmap1Bpp, top: usize, scale: usize, fg: (u8, u8, u8)) {
        let pixel = self.encode(fg.0, fg.1, fg.2);
        // Checked arithmetic: a hostile/huge bitmap or a wild `top` must fail
        // closed, not wrap into "in bounds" (review finding, M0).
        let (Some(out_width), Some(out_height)) =
            (bitmap.width.checked_mul(scale), bitmap.height.checked_mul(scale))
        else {
            return;
        };
        let Some(bottom) = top.checked_add(out_height) else {
            return;
        };
        if out_width > self.width || bottom > self.height {
            return;
        }
        let left = (self.width - out_width) / 2;
        let bytes_per_row = bitmap.width.div_ceil(8);
        for row in 0..bitmap.height {
            for column in 0..bitmap.width {
                let byte = bitmap.data[row * bytes_per_row + column / 8];
                if byte & (0x80 >> (column % 8)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            self.put(
                                left + column * scale + dx,
                                top + row * scale + dy,
                                pixel,
                            );
                        }
                    }
                }
            }
        }
    }
}
