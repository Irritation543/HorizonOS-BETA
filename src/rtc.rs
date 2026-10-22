//! Часы реального времени (CMOS RTC). Раз в секунду RTC вызывает прерывание
//! (IRQ8), по нему главный цикл обновляет часы.
use crate::port::{inb, outb};
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Time {
    pub h: u8,
    pub m: u8,
}

static TICK: AtomicBool = AtomicBool::new(false);

/// Прочитать регистр CMOS. Индекс и данные идут парой портов, поэтому
/// прерывания на время выключаются (обработчик RTC тоже трогает эти порты).
fn cmos(reg: u8) -> u8 {
    let flags: u64;
    unsafe {
        asm!("pushfq", "pop {}", "cli", out(reg) flags, options(nomem));
    }
    let value = unsafe {
        outb(0x70, reg);
        inb(0x71)
    };
    if flags & 0x200 != 0 {
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
    value
}

fn bcd(v: u8) -> u8 {
    (v & 0x0F) + (v >> 4) * 10
}

/// Часы, минуты, регистр состояния B (читаем, когда нет обновления).
fn raw() -> (u8, u8, u8) {
    for _ in 0..100_000 {
        if cmos(0x0A) & 0x80 == 0 {
            break;
        }
    }
    (cmos(0x04), cmos(0x02), cmos(0x0B))
}

pub fn read() -> Time {
    let mut a = raw();
    for _ in 0..4 {
        let b = raw();
        if a == b {
            break;
        }
        a = b;
    }
    let (mut h, mut m, status) = a;
    let pm = h & 0x80 != 0;
    h &= 0x7F;
    if status & 0x04 == 0 {
        // значения в BCD
        h = bcd(h);
        m = bcd(m);
    }
    if status & 0x02 == 0 {
        // 12-часовой режим
        h %= 12;
        if pm {
            h += 12;
        }
    }
    Time { h: h % 24, m: m % 60 }
}

/// Включить прерывание «обновление времени завершено» (раз в секунду).
pub fn enable_tick() {
    let flags: u64;
    unsafe {
        asm!("pushfq", "pop {}", "cli", out(reg) flags, options(nomem));
        outb(0x70, 0x0B);
        let b = inb(0x71);
        outb(0x70, 0x0B);
        outb(0x71, b | 0x10);
    }
    let _ = cmos(0x0C); // сбросить возможное отложенное прерывание
    if flags & 0x200 != 0 {
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
}

/// IRQ8. Без чтения регистра C RTC больше не пришлёт ни одного прерывания.
pub fn on_interrupt() {
    if cmos(0x0C) & 0x10 != 0 {
        TICK.store(true, Ordering::Release);
    }
}

/// Пришла ли секунда с прошлого вызова.
pub fn take_tick() -> bool {
    TICK.swap(false, Ordering::AcqRel)
}

pub fn tick_pending() -> bool {
    TICK.load(Ordering::Acquire)
}
