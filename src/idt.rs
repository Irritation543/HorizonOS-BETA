//! IDT и диспетчер прерываний.
use crate::gdt::KERNEL_CODE;
use crate::racy::Racy;
use crate::{apic, bsod, ps2, rtc};
use core::arch::asm;

pub const VEC_KEYBOARD: u64 = 0x21;
pub const VEC_RTC: u64 = 0x28;
pub const VEC_MOUSE: u64 = 0x2C;
pub const VEC_SPURIOUS: u64 = 0xFF;

/// Сохранённое состояние процессора; порядок полей = порядок push в isr.rs.
#[repr(C)]
pub struct Frame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Gate {
    low: u16,
    selector: u16,
    ist: u8,
    attr: u8,
    mid: u16,
    high: u32,
    zero: u32,
}

impl Gate {
    const MISSING: Gate = Gate { low: 0, selector: 0, ist: 0, attr: 0, mid: 0, high: 0, zero: 0 };

    fn new(handler: usize, ist: u8) -> Gate {
        Gate {
            low: handler as u16,
            selector: KERNEL_CODE,
            ist,
            attr: 0x8E, // present, DPL 0, interrupt gate (IF сбрасывается)
            mid: (handler >> 16) as u16,
            high: (handler >> 32) as u32,
            zero: 0,
        }
    }
}

#[repr(C, packed)]
struct DescriptorPtr {
    limit: u16,
    base: u64,
}

extern "C" {
    static ISR_TABLE: [usize; 32];
    static IRQ_TABLE: [usize; 4];
}

static IDT: Racy<[Gate; 256]> = Racy::new([Gate::MISSING; 256]);

pub fn init() {
    unsafe {
        let idt = &mut *IDT.get();
        for i in 0..32 {
            // двойная ошибка (8) работает на своём стеке IST1
            let ist = if i == 8 { 1 } else { 0 };
            idt[i] = Gate::new(ISR_TABLE[i], ist);
        }
        idt[VEC_KEYBOARD as usize] = Gate::new(IRQ_TABLE[0], 0);
        idt[VEC_RTC as usize] = Gate::new(IRQ_TABLE[1], 0);
        idt[VEC_MOUSE as usize] = Gate::new(IRQ_TABLE[2], 0);
        idt[VEC_SPURIOUS as usize] = Gate::new(IRQ_TABLE[3], 0);

        let ptr = DescriptorPtr {
            limit: (core::mem::size_of::<[Gate; 256]>() - 1) as u16,
            base: IDT.get() as u64,
        };
        asm!("lidt [{}]", in(reg) core::ptr::addr_of!(ptr), options(readonly, nostack, preserves_flags));
    }
}

/// Вызывается из isr.rs для каждого прерывания.
#[no_mangle]
extern "C" fn interrupt_dispatch(frame: &mut Frame) {
    match frame.vector {
        0..=31 => bsod::exception(frame),
        VEC_KEYBOARD => {
            ps2::on_keyboard();
            apic::eoi();
        }
        VEC_MOUSE => {
            ps2::on_mouse();
            apic::eoi();
        }
        VEC_RTC => {
            rtc::on_interrupt();
            apic::eoi();
        }
        _ => {} // ложное прерывание: EOI не нужен
    }
}
