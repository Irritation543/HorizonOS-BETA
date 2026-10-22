use crate::port::outb;
use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicUsize, Ordering};

static LAPIC: AtomicUsize = AtomicUsize::new(0);
static IOAPIC: AtomicUsize = AtomicUsize::new(0);

const IA32_APIC_BASE: u32 = 0x1B;
const IOAPIC_PHYS: u64 = 0xFEC0_0000;

fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
    }
    ((hi as u64) << 32) | lo as u64
}

fn lapic_read(reg: usize) -> u32 {
    unsafe { read_volatile((LAPIC.load(Ordering::Relaxed) + reg) as *const u32) }
}

fn lapic_write(reg: usize, value: u32) {
    unsafe { write_volatile((LAPIC.load(Ordering::Relaxed) + reg) as *mut u32, value) }
}

fn ioapic_read(reg: u32) -> u32 {
    let base = IOAPIC.load(Ordering::Relaxed);
    unsafe {
        write_volatile(base as *mut u32, reg);
        read_volatile((base + 0x10) as *const u32)
    }
}

fn ioapic_write(reg: u32, value: u32) {
    let base = IOAPIC.load(Ordering::Relaxed);
    unsafe {
        write_volatile(base as *mut u32, reg); // IOREGSEL
        write_volatile((base + 0x10) as *mut u32, value); // IOWIN
    }
}

fn route(gsi: u32, vector: u8, dest: u32) {
    ioapic_write(0x10 + gsi * 2 + 1, dest << 24);
    ioapic_write(0x10 + gsi * 2, vector as u32);
}

fn disable_pic() {
    fn out(port: u16, value: u8) {
        unsafe {
            outb(port, value);
            outb(0x80, 0);
        }
    }
    out(0x20, 0x11);
    out(0xA0, 0x11);
    out(0x21, 0x20);
    out(0xA1, 0x28);
    out(0x21, 0x04);
    out(0xA1, 0x02);
    out(0x21, 0x01);
    out(0xA1, 0x01);
    out(0x21, 0xFF);
    out(0xA1, 0xFF);
}

pub fn init(hhdm: u64) {
    disable_pic();

    let base_msr = rdmsr(IA32_APIC_BASE);
    if base_msr & (1 << 10) != 0 {
        crate::bsod::show(format_args!("Процессор в режиме x2APIC, он пока не поддерживается."));
    }
    LAPIC.store((hhdm + (base_msr & 0x000F_FFFF_FFFF_F000)) as usize, Ordering::Relaxed);
    IOAPIC.store((hhdm + IOAPIC_PHYS) as usize, Ordering::Relaxed);

    lapic_write(0x80, 0);
    lapic_write(0xF0, 0x100 | crate::idt::VEC_SPURIOUS as u32); // LAPIC
    let id = lapic_read(0x20) >> 24;

    route(1, crate::idt::VEC_KEYBOARD as u8, id);
    route(8, crate::idt::VEC_RTC as u8, id);
    route(12, crate::idt::VEC_MOUSE as u8, id);
    let max_gsi = (ioapic_read(0x01) >> 16) & 0xFF;
    for gsi in 0..=max_gsi {
        if gsi == 1 || gsi == 8 || gsi == 12 {
            continue;
        }
        ioapic_write(0x10 + gsi * 2, 1 << 16);
    }
}

pub fn eoi() {
    lapic_write(0xB0, 0);
}
