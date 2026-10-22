//! Графика: двойная буферизация, прямоугольники, 3D-рамки Win98, текст, курсор.
//! Сцена рисуется в задний буфер, на экран копируется целиком или
//! прямоугольниками. Курсор рисуется прямо в видеопамять поверх сцены.
use crate::cursor::Cursor;
use crate::font_data::{GLYPHS, GLYPH_W};
use crate::limine::RawFramebuffer;
use core::ptr::copy_nonoverlapping;

// Цвета Windows 98 (формат 0x00RRGGBB)
pub const FACE: u32 = 0xC0C0C0;
pub const WHITE: u32 = 0xFFFFFF;
pub const LIGHT: u32 = 0xDFDFDF;
pub const SHADOW: u32 = 0x808080;
pub const DARK: u32 = 0x000000;
pub const NAVY: u32 = 0x000080;
pub const DESKTOP: u32 = 0x008080;

/// Ширина «шага» между символами при выводе текста: ширина глифа + 1px
/// промежуток. Буквы в 8-пиксельной сетке шрифта стоят впритык друг к
/// другу без этого — используй эту константу везде, где считаешь ширину
/// строки в пикселях (центрирование, обрезка, ширина кнопок).
pub const CHAR_W: i32 = GLYPH_W as i32 + 1;

pub struct Screen {
    fb: *mut u8,
    pitch: usize,
    /// Куда рисуют функции ниже: задний буфер или (для синего экрана) сама видеопамять.
    back: *mut u32,
    back_stride: usize,
    pub w: i32,
    pub h: i32,
}

fn check(fb: &RawFramebuffer) -> Option<(i32, i32)> {
    // Ожидается 32 бита на пиксель в порядке B, G, R, X (обычный GOP).
    if fb.bpp != 32 || fb.width == 0 || fb.height == 0 || fb.width > 16384 || fb.height > 16384 {
        None
    } else {
        Some((fb.width as i32, fb.height as i32))
    }
}

impl Screen {
    /// Экран с задним буфером `back` на w*h пикселей.
    pub fn new(fb: &RawFramebuffer, back: *mut u32) -> Option<Screen> {
        let (w, h) = check(fb)?;
        Some(Screen { fb: fb.address, pitch: fb.pitch as usize, back, back_stride: w as usize, w, h })
    }

    /// Рисование прямо в видеопамять, без буфера (для синего экрана: не требует кучи).
    pub fn direct(fb: &RawFramebuffer) -> Option<Screen> {
        let (w, h) = check(fb)?;
        Some(Screen {
            fb: fb.address,
            pitch: fb.pitch as usize,
            back: fb.address as *mut u32,
            back_stride: fb.pitch as usize / 4,
            w,
            h,
        })
    }

    // ---------------------------------------------------------- рисование

    fn put(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            unsafe { *self.back.add(y as usize * self.back_stride + x as usize) = c };
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let len = (x1 - x0) as usize;
        for yy in y0..y1 {
            let start = yy as usize * self.back_stride + x0 as usize;
            let row = unsafe { core::slice::from_raw_parts_mut(self.back.add(start), len) };
            row.fill(color);
        }
    }

    /// Рамка в 1 пиксель: сверху/слева цвет `tl`, снизу/справа цвет `br`.
    fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, tl: u32, br: u32) {
        self.fill_rect(x, y, w - 1, 1, tl);
        self.fill_rect(x, y, 1, h - 1, tl);
        self.fill_rect(x + w - 1, y, 1, h, br);
        self.fill_rect(x, y + h - 1, w, 1, br);
    }

    /// Выпуклая кнопка/панель: светлые линии сверху-слева, тёмные снизу-справа.
    pub fn raised2(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.fill_rect(x, y, w, h, FACE);
        self.frame(x, y, w, h, WHITE, DARK);
        self.frame(x + 1, y + 1, w - 2, h - 2, LIGHT, SHADOW);
    }

    /// Вдавленная кнопка (нажатая).
    pub fn sunken2(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.fill_rect(x, y, w, h, FACE);
        self.frame(x, y, w, h, SHADOW, WHITE);
        self.frame(x + 1, y + 1, w - 2, h - 2, DARK, LIGHT);
    }

    /// Тонкая вдавленная рамка (область часов).
    pub fn sunken1(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.fill_rect(x, y, w, h, FACE);
        self.frame(x, y, w, h, SHADOW, WHITE);
    }

    // ---------------------------------------------------------- текст

    pub fn draw_char(&mut self, x: i32, y: i32, ch: char, color: u32) {
        let idx = match GLYPHS.binary_search_by_key(&(ch as u32), |&(k, _)| k) {
            Ok(i) => i,
            Err(_) => return,
        };
        for (ry, bits) in GLYPHS[idx].1.iter().enumerate() {
            for rx in 0..GLYPH_W as i32 {
                let mask = 0x80u8 >> rx;
                if *bits & mask != 0 {
                    self.put(x + rx, y + ry as i32, color);
                }
            }
        }
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, color: u32) {
        let mut cx = x;
        for ch in s.chars() {
            self.draw_char(cx, y, ch, color);
            cx += CHAR_W;
        }
    }

    /// «Жирный» текст двойным проходом со сдвигом на пиксель.
    pub fn text_bold(&mut self, x: i32, y: i32, s: &str, color: u32) {
        self.text(x, y, s, color);
        self.text(x + 1, y, s, color);
    }

    /// Текст неактивной кнопки: светлая копия со сдвигом + тёмная поверх —
    /// классический «врезанный» вид недоступных элементов в Win98.
    pub fn text_disabled(&mut self, x: i32, y: i32, s: &str) {
        self.text(x + 1, y + 1, s, WHITE);
        self.text(x, y, s, SHADOW);
    }

    /// Нарисовать маленькую картинку (иконку) в заднем буфере — как часть
    /// сцены, в отличие от курсора, который рисуется поверх неё в present.
    pub fn blit_icon(&mut self, c: &Cursor, x: i32, y: i32) {
        for cy in 0..c.h {
            let py = y + cy as i32;
            if py < 0 || py >= self.h {
                continue;
            }
            for cx in 0..c.w {
                let pxc = x + cx as i32;
                if pxc < 0 || pxc >= self.w {
                    continue;
                }
                let px = c.pixels[cy * crate::cursor::MAX + cx];
                let a = px >> 24;
                if a == 0 {
                    continue;
                }
                let idx = py as usize * self.back_stride + pxc as usize;
                unsafe {
                    let dst = *self.back.add(idx);
                    *self.back.add(idx) = if a == 255 { px & 0x00FF_FFFF } else { blend(dst, px, a) };
                }
            }
        }
    }

    /// Затемнить всю сцену к серому (фон под модальным окном).
    pub fn dim_all(&mut self, alpha: u8) {
        for y in 0..self.h {
            let row = y as usize * self.back_stride;
            for x in 0..self.w as usize {
                unsafe {
                    let p = self.back.add(row + x);
                    *p = blend(*p, 0x0080_8080, alpha as u32);
                }
            }
        }
    }

    /// Нарисовать картинку загрузки: первые 8 байт — ширина и высота
    /// (u32 LE каждая), дальше сырые RGB-тройки, без сжатия и без формата —
    /// декодировать в ядре нечего, картинка уже готова заранее. Рисуется
    /// по центру и обрезается, если реальный экран меньше картинки.
    pub fn blit_rgb888(&mut self, data: &[u8]) {
        if data.len() < 8 {
            return;
        }
        let iw = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as i32;
        let ih = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as i32;
        if iw <= 0 || ih <= 0 {
            return;
        }
        let pixels = &data[8..];
        if pixels.len() < (iw as usize) * (ih as usize) * 3 {
            return; // файл короче, чем обещает заголовок — не рискуем читать за край
        }
        let ox = (self.w - iw) / 2;
        let oy = (self.h - ih) / 2;
        for y in 0..ih {
            for x in 0..iw {
                let i = (y as usize * iw as usize + x as usize) * 3;
                let color = (pixels[i] as u32) << 16 | (pixels[i + 1] as u32) << 8 | pixels[i + 2] as u32;
                self.put(ox + x, oy + y, color); // put() сам обрежет по границам экрана
            }
        }
    }

    // ---------------------------------------------------------- вывод на экран

    /// Скопировать прямоугольник из заднего буфера в видеопамять.
    pub fn present_rect(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let len = (x1 - x0) as usize;
        for yy in y0..y1 {
            unsafe {
                let src = self.back.add(yy as usize * self.back_stride + x0 as usize);
                let dst = self.fb.add(yy as usize * self.pitch + x0 as usize * 4) as *mut u32;
                copy_nonoverlapping(src, dst, len);
            }
        }
    }

    pub fn present_all(&mut self) {
        let (w, h) = (self.w, self.h);
        self.present_rect(0, 0, w, h);
    }

    /// Нарисовать курсор прямо в видеопамять (позиция мыши = горячая точка).
    pub fn draw_cursor(&mut self, c: &Cursor, mx: i32, my: i32) {
        let ox = mx - c.hot_x;
        let oy = my - c.hot_y;
        for cy in 0..c.h {
            let y = oy + cy as i32;
            if y < 0 || y >= self.h {
                continue;
            }
            for cx in 0..c.w {
                let x = ox + cx as i32;
                if x < 0 || x >= self.w {
                    continue;
                }
                let px = c.pixels[cy * crate::cursor::MAX + cx];
                let a = px >> 24;
                if a == 0 {
                    continue;
                }
                unsafe {
                    let dst = self.fb.add(y as usize * self.pitch + x as usize * 4) as *mut u32;
                    *dst = if a == 255 { px & 0x00FF_FFFF } else { blend(*dst, px, a) };
                }
            }
        }
    }
}

fn blend(dst: u32, src: u32, a: u32) -> u32 {
    let mix = |s: u32, d: u32| (s * a + d * (255 - a)) / 255;
    let r = mix((src >> 16) & 255, (dst >> 16) & 255);
    let g = mix((src >> 8) & 255, (dst >> 8) & 255);
    let b = mix(src & 255, dst & 255);
    (r << 16) | (g << 8) | b
}
