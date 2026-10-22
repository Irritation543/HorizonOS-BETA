//! Курсоры мыши. Читаем настоящий формат .cur/.ico (1 бит и 32 бита),
//! поэтому тот же разбор пригодится позже для иконок с диска.
use core::cell::UnsafeCell;

pub const MAX: usize = 32;

#[derive(Clone, Copy)]
pub struct Cursor {
    pub w: usize,
    pub h: usize,
    /// «Горячая точка»: пиксель, который указывает на позицию мыши.
    pub hot_x: i32,
    pub hot_y: i32,
    /// ARGB, строка = MAX пикселей. Альфа 0 = прозрачный.
    pub pixels: [u32; MAX * MAX],
}

impl Cursor {
    pub const EMPTY: Cursor = Cursor {
        w: 0,
        h: 0,
        hot_x: 0,
        hot_y: 0,
        pixels: [0; MAX * MAX],
    };
}

// Порядок = клавиши 1..8 в тестовом режиме.
static FILES: [&[u8]; 8] = [
    include_bytes!("../assets/cursors/default.cur"),
    include_bytes!("../assets/cursors/app_starting.cur"),
    include_bytes!("../assets/cursors/busy.cur"),
    include_bytes!("../assets/cursors/hand.cur"),
    include_bytes!("../assets/cursors/size_we.cur"),
    include_bytes!("../assets/cursors/size_ns.cur"),
    include_bytes!("../assets/cursors/size_nesw.cur"),
    include_bytes!("../assets/cursors/size_nwse.cur"),
];

struct Store(UnsafeCell<[Cursor; 8]>);
unsafe impl Sync for Store {}
static STORE: Store = Store(UnsafeCell::new([Cursor::EMPTY; 8]));

/// Разобрать все встроенные курсоры (вызывается один раз при старте).
pub fn load_all() {
    for (i, data) in FILES.iter().enumerate() {
        if let Some(c) = parse(data) {
            unsafe { (*STORE.0.get())[i] = c };
        }
    }
}

pub fn get(i: usize) -> &'static Cursor {
    unsafe { &(*STORE.0.get())[i % 8] }
}

fn rd16(d: &[u8], o: usize) -> Option<u32> {
    let b = d.get(o..o + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]) as u32)
}

fn rd32(d: &[u8], o: usize) -> Option<u32> {
    let b = d.get(o..o + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Разбор .cur / .ico. Берётся первая картинка в файле.
pub fn parse(d: &[u8]) -> Option<Cursor> {
    // ICONDIR: reserved=0, type (1 = ico, 2 = cur), count
    if rd16(d, 0)? != 0 {
        return None;
    }
    let kind = rd16(d, 2)?;
    if (kind != 1 && kind != 2) || rd16(d, 4)? == 0 {
        return None;
    }
    // ICONDIRENTRY (с офсета 6): для .cur слова 10 и 12 = горячая точка
    let (hot_x, hot_y) = if kind == 2 {
        (rd16(d, 10)? as i32, rd16(d, 12)? as i32)
    } else {
        (0, 0)
    };
    let off = rd32(d, 18)? as usize;

    // BITMAPINFOHEADER; высота хранится удвоенной (картинка + маска)
    let hdr = rd32(d, off)? as usize;
    let w = rd32(d, off + 4)? as usize;
    let h = rd32(d, off + 8)? as usize / 2;
    let bpp = rd16(d, off + 14)?;
    if rd32(d, off + 16)? != 0 || w == 0 || h == 0 || w > MAX || h > MAX {
        return None;
    }

    let mask_row = (w + 31) / 32 * 4;
    let (color_off, color_row) = match bpp {
        1 => (off + hdr + 8, mask_row), // после палитры из 2 цветов
        32 => (off + hdr, w * 4),
        _ => return None,
    };
    let and_off = color_off + color_row * h;

    let mut has_alpha = false;
    if bpp == 32 {
        for y in 0..h {
            for x in 0..w {
                if rd32(d, color_off + y * color_row + x * 4)? >> 24 != 0 {
                    has_alpha = true;
                }
            }
        }
    }

    let mut c = Cursor::EMPTY;
    c.w = w;
    c.h = h;
    c.hot_x = hot_x;
    c.hot_y = hot_y;

    for y in 0..h {
        let src = h - 1 - y; // строки в файле идут снизу вверх
        for x in 0..w {
            let masked = (*d.get(and_off + src * mask_row + x / 8)? >> (7 - x % 8)) & 1 != 0;
            let px = if bpp == 1 {
                let bit = ((*d.get(color_off + src * color_row + x / 8)? >> (7 - x % 8)) & 1) as usize;
                let p = off + hdr + bit * 4; // запись палитры: B, G, R, 0
                if masked {
                    0
                } else {
                    0xFF00_0000
                        | (*d.get(p + 2)? as u32) << 16
                        | (*d.get(p + 1)? as u32) << 8
                        | *d.get(p)? as u32
                }
            } else {
                let v = rd32(d, color_off + src * color_row + x * 4)?;
                if has_alpha {
                    v
                } else if masked {
                    0
                } else {
                    v | 0xFF00_0000
                }
            };
            c.pixels[y * MAX + x] = px;
        }
    }
    Some(c)
}
