
use crate::font_data::GLYPH_H;
use crate::gfx::{Screen, CHAR_W, FACE, WHITE};
use crate::idt::Frame;
use crate::power;
use core::arch::asm;
use core::fmt::{self, Write};

const BLUE: u32 = 0x0000AA;
const MARGIN: i32 = 32;

const NAMES: [&str; 32] = [
    "Деление на ноль",
    "Отладка",
    "Немаскируемое прерывание",
    "Точка останова",
    "Переполнение",
    "Выход за границы",
    "Недопустимая инструкция",
    "Сопроцессор недоступен",
    "Двойная ошибка",
    "Ошибка сегмента сопроцессора",
    "Неверный TSS",
    "Сегмент отсутствует",
    "Ошибка сегмента стека",
    "Нарушение общей защиты",
    "Ошибка страницы",
    "Зарезервировано",
    "Ошибка сопроцессора",
    "Ошибка выравнивания",
    "Аппаратная ошибка",
    "Ошибка SIMD",
    "Ошибка виртуализации",
    "Ошибка защиты потока",
    "Зарезервировано",
    "Зарезервировано",
    "Зарезервировано",
    "Зарезервировано",
    "Зарезервировано",
    "Зарезервировано",
    "Внедрение гипервизора",
    "Ошибка VMM",
    "Ошибка безопасности",
    "Зарезервировано",
];

struct Console {
    screen: Screen,
    x: i32,
    y: i32,
}

impl Console {
    fn newline(&mut self) {
        self.x = MARGIN;
        self.y += GLYPH_H as i32;
    }
}

impl Write for Console {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for ch in text.chars() {
            if ch == '\n' {
                self.newline();
                continue;
            }
            // перенос строки, если символ не помещается
            if self.x + CHAR_W > self.screen.w - MARGIN {
                self.newline();
            }
            self.screen.draw_char(self.x, self.y, ch, WHITE);
            self.x += CHAR_W;
        }
        Ok(())
    }
}

/// Показать синий экран с сообщением и остановить систему.
pub fn show(args: fmt::Arguments) -> ! {
    unsafe { asm!("cli", options(nomem, nostack)) };
    if let Some(fb) = crate::FRAMEBUFFER.first() {
        if let Some(mut screen) = Screen::direct(fb) {
            let (w, h) = (screen.w, screen.h);
            screen.fill_rect(0, 0, w, h, BLUE);

            // заголовок по центру: серая плашка с синим текстом
            let title = " HorizonOS ";
            let tw = title.len() as i32 * CHAR_W;
            let tx = (w - tw) / 2;
            screen.fill_rect(tx, 40, tw, GLYPH_H as i32, FACE);
            screen.text(tx, 40, title, BLUE);

            let mut console = Console { screen, x: MARGIN, y: 80 };
            let _ = console.write_fmt(args);
            let _ = console.write_str("\n\nСистема остановлена. Перезагрузите компьютер.");
        }
    }
    power::halt()
}

/// Исключение процессора.
pub fn exception(frame: &Frame) -> ! {
    let name = NAMES[(frame.vector & 31) as usize];
    if frame.vector == 14 {
        let cr2: u64;
        unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags)) };
        show(format_args!(
            "Критическая ошибка: {} (#{})\nАдрес обращения: {:#x}\nRIP: {:#x}\nКод ошибки: {:#x}",
            name, frame.vector, cr2, frame.rip, frame.error
        ))
    } else {
        show(format_args!(
            "Критическая ошибка: {} (#{})\nRIP: {:#x}\nКод ошибки: {:#x}",
            name, frame.vector, frame.rip, frame.error
        ))
    }
}
