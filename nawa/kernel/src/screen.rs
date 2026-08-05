//! The screen — what i3mlOS shows a person, rather than what it tells a log.
//!
//! Every other operating system boots into a place to type commands. This one
//! boots into an account of itself: what it was asked to do, what it did,
//! what it spent, and what it is waiting for permission to do next. The
//! serial log is for machines; this is the same information for the person
//! whose machine it is.
//!
//! Deliberately plain: an 8x16 grid, three colours, no compositor. The
//! consent card is drawn last and framed, because the one thing on this
//! screen that must never be missed is the question.

use nawa_core::fb::{Bitmap1Bpp, Framebuffer};

use crate::font;

/// A single lit pixel, as a bitmap — the primitive rules and frames are drawn
/// from. `'static` because a `Bitmap1Bpp` borrows its bytes.
static PIXEL: [u8; 1] = [0xff];

pub const INK: (u8, u8, u8) = (0xf5, 0xf0, 0xe6);
pub const DIM: (u8, u8, u8) = (0x7c, 0x87, 0xa8);
pub const ACCENT: (u8, u8, u8) = (0x1a, 0x86, 0xee);
pub const WARN: (u8, u8, u8) = (0xf2, 0xb0, 0x3a);
pub const BACKGROUND: (u8, u8, u8) = (0x0b, 0x10, 0x21);

pub struct Screen {
    framebuffer: Framebuffer,
    /// Where the next line goes, in pixels.
    cursor_y: usize,
    pub left: usize,
}

impl Screen {
    pub fn new(framebuffer: Framebuffer) -> Screen {
        framebuffer.clear(BACKGROUND.0, BACKGROUND.1, BACKGROUND.2);
        Screen { framebuffer, cursor_y: 0, left: 64 }
    }

    pub fn width(&self) -> usize {
        self.framebuffer.width
    }

    #[allow(dead_code)]
    pub fn height(&self) -> usize {
        self.framebuffer.height
    }

    pub fn set_y(&mut self, y: usize) {
        self.cursor_y = y;
    }

    pub fn y(&self) -> usize {
        self.cursor_y
    }

    /// Draw one glyph. Bounds are the framebuffer's problem, not ours — it
    /// refuses anything outside itself.
    fn glyph(&self, byte: u8, x: usize, y: usize, colour: (u8, u8, u8)) {
        let rows = font::glyph(byte);
        let bitmap = Bitmap1Bpp { width: font::CELL_WIDTH, height: font::CELL_HEIGHT, data: rows };
        self.framebuffer.blit(&bitmap, x, y, 1, colour);
    }

    pub fn text_at(&self, text: &str, x: usize, y: usize, colour: (u8, u8, u8)) -> usize {
        let mut cursor = x;
        for byte in text.bytes() {
            // Multi-byte characters would need shaping; until that exists,
            // anything outside ASCII is left as a gap rather than mangled.
            self.glyph(byte, cursor, y, colour);
            cursor += font::CELL_WIDTH;
            if cursor + font::CELL_WIDTH > self.framebuffer.width {
                break;
            }
        }
        cursor
    }

    /// A line of text, advancing down the page.
    pub fn line(&mut self, text: &str, colour: (u8, u8, u8)) {
        self.text_at(text, self.left, self.cursor_y, colour);
        self.cursor_y += font::CELL_HEIGHT + 4;
    }

    pub fn blank(&mut self, pixels: usize) {
        self.cursor_y += pixels;
    }

    /// A filled rule, for separating what the machine did from what it wants.
    pub fn rule(&mut self, colour: (u8, u8, u8)) {
        let bitmap = Bitmap1Bpp { width: 1, height: 1, data: &PIXEL };
        let width = self.framebuffer.width.saturating_sub(self.left * 2);
        for x in 0..width {
            self.framebuffer.blit(&bitmap, self.left + x, self.cursor_y, 1, colour);
        }
        self.cursor_y += 12;
    }

    pub fn blit_centered(&self, bitmap: &Bitmap1Bpp, top: usize, scale: usize, colour: (u8, u8, u8)) {
        self.framebuffer.blit_centered(bitmap, top, scale, colour);
    }

    pub fn blit(&self, bitmap: &Bitmap1Bpp, x: usize, y: usize, colour: (u8, u8, u8)) {
        self.framebuffer.blit(bitmap, x, y, 1, colour);
    }

    /// The consent card: a frame, the kernel's description of what is being
    /// asked for, and the agent's own words marked as the agent's.
    ///
    /// The frame is not decoration. This is the one element on the screen a
    /// person must not skim past, and from Phase 3 it is drawn by a
    /// compositor that no agent can address — the frame is where that
    /// boundary will become literal.
    pub fn consent_card(&mut self, kernel_says: &str, agent_says: &str, agent: u32) {
        let top = self.cursor_y;
        let width = self.framebuffer.width.saturating_sub(self.left * 2);
        let height = font::CELL_HEIGHT * 4 + 28;

        let dot = Bitmap1Bpp { width: 1, height: 1, data: &PIXEL };
        for x in 0..width {
            self.framebuffer.blit(&dot, self.left + x, top, 1, WARN);
            self.framebuffer.blit(&dot, self.left + x, top + height, 1, WARN);
        }
        for y in 0..height {
            self.framebuffer.blit(&dot, self.left, top + y, 1, WARN);
            self.framebuffer.blit(&dot, self.left + width - 1, top + y, 1, WARN);
        }

        let inner = self.left + 16;
        let mut y = top + 12;
        self.text_at("WAITING FOR YOU", inner, y, WARN);
        y += font::CELL_HEIGHT + 6;
        let mut cursor = self.text_at("agent ", inner, y, DIM);
        let mut number = [b'0'; 4];
        let digits = format_number(agent as u64, &mut number);
        cursor = self.text_at(digits, cursor, y, ACCENT);
        cursor = self.text_at(" wants to: ", cursor, y, DIM);
        self.text_at(kernel_says, cursor, y, INK);
        y += font::CELL_HEIGHT + 6;
        let cursor = self.text_at("it says: \"", inner, y, DIM);
        let cursor = self.text_at(agent_says, cursor, y, DIM);
        self.text_at("\"", cursor, y, DIM);

        self.cursor_y = top + height + 20;
    }
}

/// Decimal, into a caller's buffer. The screen must be able to say a number
/// when the heap is gone.
pub fn format_number(mut value: u64, buffer: &mut [u8]) -> &str {
    if buffer.is_empty() {
        return "";
    }
    if value == 0 {
        buffer[0] = b'0';
        return core::str::from_utf8(&buffer[..1]).unwrap_or("0");
    }
    let mut digits = [0u8; 20];
    let mut length = 0;
    while value > 0 && length < digits.len() {
        digits[length] = b'0' + (value % 10) as u8;
        value /= 10;
        length += 1;
    }
    let take = length.min(buffer.len());
    for index in 0..take {
        buffer[index] = digits[length - 1 - index];
    }
    core::str::from_utf8(&buffer[..take]).unwrap_or("")
}
