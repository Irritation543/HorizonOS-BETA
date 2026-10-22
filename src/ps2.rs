//! Драйвер контроллера PS/2 (i8042): клавиатура и мышь, по прерываниям IRQ1 и IRQ12.
//! Обработчики разбирают байты и кладут события в очередь (events.rs).
use crate::events;
use crate::port::{inb, outb};
use crate::racy::Racy;

const DATA: u16 = 0x60;
const STATUS: u16 = 0x64;

#[derive(Clone, Copy)]
pub enum Event {
    /// Скан-код набора 1. `extended` = префикс 0xE0 (стрелки, Win, правые Ctrl/Alt).
    Key { code: u8, extended: bool, pressed: bool },
    /// Смещение мыши (вверх = отрицательное, как на экране) и кнопки: 1 левая, 2 правая, 4 средняя.
    Mouse { dx: i32, dy: i32, buttons: u8 },
}

/// Состояние разбора байтов. Клавиатурная и мышиная части не пересекаются.
struct State {
    ext: bool,
    skip: u8,
    mouse_buf: [u8; 3],
    mouse_idx: usize,
}

static STATE: Racy<State> = Racy::new(State { ext: false, skip: 0, mouse_buf: [0; 3], mouse_idx: 0 });

fn wait_write() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(STATUS) } & 0x02 == 0 {
            return true;
        }
    }
    false
}

fn wait_read() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(STATUS) } & 0x01 != 0 {
            return true;
        }
    }
    false
}

fn command(c: u8) {
    if wait_write() {
        unsafe { outb(STATUS, c) };
    }
}

fn write_data(d: u8) {
    if wait_write() {
        unsafe { outb(DATA, d) };
    }
}

fn read_data() -> Option<u8> {
    if wait_read() {
        Some(unsafe { inb(DATA) })
    } else {
        None
    }
}

fn flush() {
    for _ in 0..64 {
        if unsafe { inb(STATUS) } & 0x01 == 0 {
            break;
        }
        unsafe { inb(DATA) };
    }
}

fn mouse_write(d: u8) {
    command(0xD4); // следующий байт уходит устройству на втором порту (мышь)
    write_data(d);
}

/// Настроить контроллер и оба устройства. Вызывать при выключенных прерываниях
/// и уже настроенном IOAPIC.
pub fn init() {
    command(0xAD); // выключить порт клавиатуры
    command(0xA7); // выключить порт мыши
    flush();

    command(0x20); // прочитать конфигурацию
    let mut cfg = read_data().unwrap_or(0);
    cfg &= !0x33; // пока без IRQ, тактирование портов включено
    cfg |= 0x40; // трансляция в scancode set 1
    command(0x60);
    write_data(cfg);

    command(0xA8); // включить порт мыши
    command(0xAE); // включить порт клавиатуры

    write_data(0xF4); // клавиатура: начать сканирование
    let _ = read_data(); // ACK (0xFA)
    mouse_write(0xF6); // мышь: настройки по умолчанию
    let _ = read_data();
    mouse_write(0xF4); // мышь: включить отправку пакетов
    let _ = read_data();
    flush();

    // теперь можно включить прерывания IRQ1 и IRQ12 в контроллере
    command(0x20);
    let cfg = read_data().unwrap_or(cfg);
    command(0x60);
    write_data(cfg | 0x03);
}

/// IRQ1.
pub fn on_keyboard() {
    if unsafe { inb(STATUS) } & 0x01 == 0 {
        return;
    }
    let data = unsafe { inb(DATA) };
    let st = unsafe { &mut *STATE.get() };
    if let Some(ev) = key_byte(st, data) {
        events::push(ev);
    }
}

/// IRQ12.
pub fn on_mouse() {
    if unsafe { inb(STATUS) } & 0x01 == 0 {
        return;
    }
    let data = unsafe { inb(DATA) };
    let st = unsafe { &mut *STATE.get() };
    if let Some(ev) = mouse_byte(st, data) {
        events::push(ev);
    }
}

fn mouse_byte(st: &mut State, data: u8) -> Option<Event> {
    // первый байт пакета всегда с битом 3; иначе потеряна синхронизация
    if st.mouse_idx == 0 && data & 0x08 == 0 {
        return None;
    }
    st.mouse_buf[st.mouse_idx] = data;
    st.mouse_idx += 1;
    if st.mouse_idx < 3 {
        return None;
    }
    st.mouse_idx = 0;
    let b = st.mouse_buf;
    if b[0] & 0xC0 != 0 {
        return None; // переполнение: пакет выбрасываем
    }
    let dx = b[1] as i32 - if b[0] & 0x10 != 0 { 256 } else { 0 };
    let dy = b[2] as i32 - if b[0] & 0x20 != 0 { 256 } else { 0 };
    Some(Event::Mouse { dx, dy: -dy, buttons: b[0] & 0x07 })
}

fn key_byte(st: &mut State, data: u8) -> Option<Event> {
    if st.skip > 0 {
        st.skip -= 1;
        return None;
    }
    match data {
        0xE0 => {
            st.ext = true;
            None
        }
        0xE1 => {
            st.skip = 5; // Pause/Break: пропускаем остаток последовательности
            None
        }
        0x00 | 0xFA | 0xFE | 0xFF => None, // ACK, повтор, ошибки
        _ => {
            let extended = core::mem::replace(&mut st.ext, false);
            Some(Event::Key {
                code: data & 0x7F,
                extended,
                pressed: data & 0x80 == 0,
            })
        }
    }
}
