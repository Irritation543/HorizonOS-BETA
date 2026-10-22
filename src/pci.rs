//! Перечисление устройств PCI через порты 0xCF8/0xCFC (конфигурационный
//! механизм №1). Работает на всех чипсетах с поддержкой legacy-PCI,
//! в том числе в QEMU на q35 и i440fx.
use crate::port::{inl, outl};

const ADDR: u16 = 0xCF8;
const DATA: u16 = 0xCFC;

fn address(bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
    0x8000_0000 | (bus as u32) << 16 | (dev as u32) << 11 | (func as u32) << 8 | (reg as u32 & 0xFC)
}

fn read32(bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
    unsafe {
        outl(ADDR, address(bus, dev, func, reg));
        inl(DATA)
    }
}

fn write32(bus: u8, dev: u8, func: u8, reg: u8, value: u32) {
    unsafe {
        outl(ADDR, address(bus, dev, func, reg));
        outl(DATA, value);
    }
}

#[derive(Clone, Copy)]
pub struct Device {
    bus: u8,
    dev: u8,
    func: u8,
}

impl Device {
    /// Прочитать BAR (Base Address Register) номер `i` (0..=5).
    pub fn bar(&self, i: u8) -> u32 {
        read32(self.bus, self.dev, self.func, 0x10 + i * 4)
    }

    /// Включить обращение к памяти устройства и bus-mastering
    /// (без этого контроллер не сможет делать DMA), и запретить устройству
    /// присылать обычное PCI-прерывание (бит 10) — оба наших драйвера
    /// (AHCI, AC97) работают опросом, обработчика для линии INTx# нет.
    pub fn enable_bus_master(&self) {
        let cmd = read32(self.bus, self.dev, self.func, 0x04);
        write32(self.bus, self.dev, self.func, 0x04, cmd | 0x0006 | (1 << 10));
    }
}

/// Найти первое устройство по классу/подклассу/программному интерфейсу.
/// AHCI: class=0x01 (накопители), subclass=0x06 (SATA), prog_if=0x01 (AHCI).
pub fn find(class: u8, subclass: u8, prog_if: u8) -> Option<Device> {
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            let header0 = read32(bus, dev, 0, 0x00);
            if header0 & 0xFFFF == 0xFFFF {
                continue; // устройства нет
            }
            let multi = (read32(bus, dev, 0, 0x0C) >> 16) & 0x80 != 0;
            let funcs = if multi { 8 } else { 1 };
            for func in 0..funcs {
                let vendor = read32(bus, dev, func, 0x00) & 0xFFFF;
                if vendor == 0xFFFF {
                    continue;
                }
                let class_reg = read32(bus, dev, func, 0x08);
                let (c, sc, pi) =
                    ((class_reg >> 24) as u8, (class_reg >> 16) as u8, (class_reg >> 8) as u8);
                if c == class && sc == subclass && pi == prog_if {
                    return Some(Device { bus, dev, func });
                }
            }
        }
    }
    None
}
