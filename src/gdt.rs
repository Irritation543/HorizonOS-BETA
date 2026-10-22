//! GDT и TSS. Отдельный стек для двойной ошибки (IST1), чтобы ядро
//! показало синий экран, даже если сломан основной стек.
use crate::racy::Racy;
use core::arch::asm;

pub const KERNEL_CODE: u16 = 0x08;
pub const KERNEL_DATA: u16 = 0x10;
const TSS_SELECTOR: u16 = 0x18;
const DF_STACK_SIZE: usize = 16 * 1024;

/// TSS: 104 байта. Поля читаются процессором, поэтому храним как u32[26].
#[repr(C, align(8))]
struct Tss([u32; 26]);

#[repr(C, align(16))]
struct Stack([u8; DF_STACK_SIZE]);

#[repr(C, packed)]
struct DescriptorPtr {
    limit: u16,
    base: u64,
}

static TSS: Racy<Tss> = Racy::new(Tss([0; 26]));
static GDT: Racy<[u64; 5]> = Racy::new([0; 5]);
static DF_STACK: Racy<Stack> = Racy::new(Stack([0; DF_STACK_SIZE]));

pub fn init() {
    unsafe {
        // IST1 = вершина стека двойной ошибки (слова 9 и 10), iomap_base = 104 (слово 25, старшая половина)
        let top = DF_STACK.get() as u64 + DF_STACK_SIZE as u64;
        let tss = &mut (*TSS.get()).0;
        tss[9] = top as u32;
        tss[10] = (top >> 32) as u32;
        tss[25] = 104 << 16;

        let base = TSS.get() as u64;
        let limit = 103u64;
        let gdt = &mut *GDT.get();
        gdt[0] = 0;
        gdt[1] = 0x00AF_9A00_0000_FFFF; // 64-битный код ядра
        gdt[2] = 0x00CF_9200_0000_FFFF; // данные ядра
        gdt[3] = (limit & 0xFFFF)
            | ((base & 0xFFFF) << 16)
            | (((base >> 16) & 0xFF) << 32)
            | (0x89u64 << 40) // TSS, доступный
            | (((base >> 24) & 0xFF) << 56);
        gdt[4] = base >> 32;

        let ptr = DescriptorPtr { limit: (5 * 8 - 1) as u16, base: GDT.get() as u64 };
        asm!("lgdt [{}]", in(reg) core::ptr::addr_of!(ptr), options(readonly, nostack, preserves_flags));

        // перезагрузить CS дальним возвратом, затем сегменты данных
        asm!(
            "push {cs}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            cs = in(reg) KERNEL_CODE as u64,
            tmp = out(reg) _,
            options(preserves_flags),
        );
        asm!(
            "mov ds, {0:x}",
            "mov es, {0:x}",
            "mov ss, {0:x}",
            "mov fs, {1:x}",
            "mov gs, {1:x}",
            in(reg) KERNEL_DATA,
            in(reg) 0u16,
            options(nostack, preserves_flags),
        );
        asm!("ltr {0:x}", in(reg) TSS_SELECTOR, options(nomem, nostack, preserves_flags));
    }
}
