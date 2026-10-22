//! Остановка, перезагрузка, выключение.
//! Пока временные способы: перезагрузка через контроллер клавиатуры,
//! выключение только для QEMU. Позже заменим на ACPI.
use crate::ac97::Ac97;
use crate::gfx::{Screen, CHAR_W};
use crate::port::{inb, outb, outw};
use core::arch::asm;

pub fn halt() -> ! {
    loop {
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

pub fn reboot() -> ! {
    unsafe {
        for _ in 0..100_000 {
            if inb(0x64) & 0x02 == 0 {
                break;
            }
        }
        outb(0x64, 0xFE); // импульс на линию reset
    }
    halt()
}

pub fn shutdown() -> ! {
    unsafe {
        outw(0x604, 0x2000); // QEMU (q35 / i440fx)
        outw(0xB004, 0x2000); // старый QEMU / Bochs
    }
    halt()
}

/// Чёрный экран с сообщением, как в Windows 98 — только рисует и показывает.
fn show_message(screen: &mut Screen, line1: &str, line2: &str) {
    let (w, h) = (screen.w, screen.h);
    screen.fill_rect(0, 0, w, h, 0x00_0000);
    let cx = |s: &str| (w - s.chars().count() as i32 * CHAR_W) / 2;
    let y = h / 2 - 16;
    screen.text(cx(line1), y, line1, 0xFF_FFFF);
    screen.text(cx(line2), y + 24, line2, 0xC0_C0C0);
    screen.present_all();
}

/// Подождать одно тиканье RTC (до секунды) — чтобы сообщение точно
/// успели увидеть, если звука не было (например, нет диска или AC97).
fn wait_tick() {
    crate::rtc::take_tick(); // сбросить возможный «старый» тик
    loop {
        unsafe { asm!("sti", "hlt", options(nomem, nostack)) };
        if crate::rtc::take_tick() {
            break;
        }
    }
}

/// Перезагрузка. Без звука — так попросили: звук только при выключении.
pub fn reboot_with_screen(screen: &mut Screen) -> ! {
    show_message(screen, "HorizonOS перезагружается", "Все приложения закрываются...");
    wait_tick();
    reboot()
}

/// Выключение с чёрным экраном и (если получится) звуком выключения.
/// Играем звук ПОСЛЕ вывода текста — так надпись видна сразу, а не
/// только после того, как звук закончится.
pub fn shutdown_with_screen(screen: &mut Screen, ac97: Option<&mut Ac97>, sound: Option<&[u8]>) -> ! {
    show_message(screen, "HorizonOS завершает работу", "Все приложения закрываются...");

    let played = match (ac97, sound) {
        (Some(ac), Some(bytes)) => match crate::wav::extract_pcm(bytes) {
            Some(pcm) => ac.play_blocking(pcm),
            None => false,
        },
        _ => false,
    };
    if !played {
        wait_tick(); // звука не было — просто небольшая пауза, чтобы надпись увидели
    }
    shutdown()
}
