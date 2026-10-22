//! Драйвер AHCI: находит первый диск на первом занятом порту и умеет
//! читать с него секторы. Один командный слот, без очередей (NCQ),
//! без записи — для диска A: пока хватает чтения.
//!
//! ВАЖНО: все буферы, которые передаются в `Disk::read` (и все буферы
//! внутри самого драйвера), обязаны быть выделены в куче (`Vec`/`Box`),
//! а не лежать на стеке. Куча целиком лежит в окне HHDM (см. heap.rs),
//! поэтому для неё верна формула «физический адрес = виртуальный − hhdm».
//! Для памяти на стеке эта формула неверна: стек ядра размечен
//! загрузчиком отдельно и по этой формуле не считается.
use crate::pci;
use alloc::boxed::Box;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

const SECTOR: usize = 512;
const MAX_SECTORS_PER_CMD: u32 = 128; // 64 КБ за одну команду

// смещения регистров HBA (порт AHCI, offset относительно ABAR)
const GHC: usize = 0x04;
const PI: usize = 0x0C;

// смещения регистров порта относительно его базы (ABAR + 0x100 + i*0x80)
const P_CLB: usize = 0x00;
const P_CLBU: usize = 0x04;
const P_FB: usize = 0x08;
const P_FBU: usize = 0x0C;
const P_IS: usize = 0x10;
const P_CMD: usize = 0x18;
const P_TFD: usize = 0x20;
const P_SIG: usize = 0x24;
const P_SSTS: usize = 0x28;
const P_SERR: usize = 0x30;
const P_CI: usize = 0x38;

const CMD_ST: u32 = 1 << 0;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;
const TFD_BSY_DRQ: u32 = 0x88;
const IS_TFES: u32 = 1 << 30; // ошибка в task file

fn r32(addr: usize) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}

fn w32(addr: usize, v: u32) {
    unsafe { write_volatile(addr as *mut u32, v) }
}

// Командный список: до 32 заголовков по 32 байта, используем только слот 0.
#[repr(C, align(1024))]
struct CmdList([u8; 1024]);

// Область приёма FIS от устройства (мы её не разбираем, но выделить обязаны).
#[repr(C, align(256))]
struct FisRecv([u8; 256]);

// Таблица команды: CFIS(64) + ACMD(16) + rsv(48) + один PRDT(16) = 144 байта.
#[repr(C, align(128))]
struct CmdTable([u8; 256]);

pub struct Disk {
    port: usize,
    hhdm: u64,
    cmdlist: Box<CmdList>,
    _fis: Box<FisRecv>,
    table: Box<CmdTable>,
}

fn phys(virt_addr: u64, hhdm: u64) -> u64 {
    virt_addr - hhdm
}

fn stop_cmd(port: usize) -> bool {
    w32(port + P_CMD, r32(port + P_CMD) & !(CMD_ST | CMD_FRE));
    for _ in 0..1_000_000 {
        if r32(port + P_CMD) & (CMD_FR | CMD_CR) == 0 {
            return true;
        }
    }
    false
}

fn start_cmd(port: usize) {
    while r32(port + P_CMD) & CMD_CR != 0 {}
    w32(port + P_CMD, r32(port + P_CMD) | CMD_FRE | CMD_ST);
}

/// Найти AHCI-контроллер на шине PCI и поднять первый диск на первом
/// подключённом и активном порту. `None`, если контроллера нет или
/// ни на одном порту нет диска (например, забыли подключить `disk.img`).
pub fn init(hhdm: u64) -> Option<Disk> {
    let dev = pci::find(0x01, 0x06, 0x01)?;
    dev.enable_bus_master();

    let bar5 = dev.bar(5);
    if bar5 & 0x1 != 0 || (bar5 >> 1) & 0x3 == 2 {
        return None; // не memory-BAR или 64-битный (у ABAR такого не бывает)
    }
    let abar = (hhdm + (bar5 & !0xF) as u64) as usize;

    w32(abar + GHC, r32(abar + GHC) | (1 << 31)); // GHC.AE: включить режим AHCI

    let pi = r32(abar + PI);
    for i in 0..32u32 {
        if pi & (1 << i) == 0 {
            continue;
        }
        let port = abar + 0x100 + i as usize * 0x80;

        let ssts = r32(port + P_SSTS);
        let det = ssts & 0xF;
        let ipm = (ssts >> 8) & 0xF;
        if det != 3 || ipm != 1 {
            continue; // нет физического подключения или связь не установлена
        }
        if r32(port + P_SIG) != 0x0000_0101 {
            continue; // не обычный SATA-диск (ATAPI, порт-мультипликатор и т.п.)
        }
        if !stop_cmd(port) {
            continue; // порт не остановился штатно — пропускаем, не трогаем регистры
        }

        let cmdlist = Box::new(CmdList([0; 1024]));
        let fis = Box::new(FisRecv([0; 256]));
        let table = Box::new(CmdTable([0; 256]));

        let clb = phys(cmdlist.0.as_ptr() as u64, hhdm);
        let fb = phys(fis.0.as_ptr() as u64, hhdm);
        w32(port + P_CLB, clb as u32);
        w32(port + P_CLBU, (clb >> 32) as u32);
        w32(port + P_FB, fb as u32);
        w32(port + P_FBU, (fb >> 32) as u32);
        w32(port + P_IS, 0xFFFF_FFFF); // запись 1 сбрасывает бит — очищаем всё
        w32(port + P_SERR, 0xFFFF_FFFF);

        start_cmd(port);

        return Some(Disk { port, hhdm, cmdlist, _fis: fis, table });
    }
    None
}

impl Disk {
    /// Прочитать `count` секторов по 512 байт начиная с LBA `lba` в `buf`.
    /// `buf` должен быть выделен в куче (см. предупреждение вверху файла)
    /// и содержать не меньше `count * 512` байт.
    pub fn read(&mut self, mut lba: u64, mut count: u32, buf: &mut [u8]) -> bool {
        let mut off = 0usize;
        while count > 0 {
            let chunk = count.min(MAX_SECTORS_PER_CMD);
            let bytes = chunk as usize * SECTOR;
            if !self.read_chunk(lba, chunk, &mut buf[off..off + bytes]) {
                return false;
            }
            off += bytes;
            lba += chunk as u64;
            count -= chunk;
        }
        true
    }

    fn read_chunk(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> bool {
        let port = self.port;

        for _ in 0..1_000_000 {
            if r32(port + P_TFD) & TFD_BSY_DRQ == 0 {
                break;
            }
        }

        let buf_phys = phys(buf.as_ptr() as u64, self.hhdm);

        // Command FIS: Register FIS Host->Device, команда READ DMA EXT (0x25).
        let t = &mut self.table.0;
        t.fill(0);
        t[0] = 0x27; // FIS_TYPE_REG_H2D
        t[1] = 0x80; // C = 1 (это команда, не просто обновление регистров)
        t[2] = 0x25; // READ DMA EXT
        t[4] = lba as u8;
        t[5] = (lba >> 8) as u8;
        t[6] = (lba >> 16) as u8;
        t[7] = 0x40; // выбор LBA-режима
        t[8] = (lba >> 24) as u8;
        t[9] = (lba >> 32) as u8;
        t[10] = (lba >> 40) as u8;
        t[12] = count as u8;
        t[13] = (count >> 8) as u8;

        // PRDT: одна запись сразу после CFIS(64) + ACMD(16) + rsv(48) = offset 128.
        const PRDT: usize = 128;
        t[PRDT..PRDT + 4].copy_from_slice(&(buf_phys as u32).to_le_bytes());
        t[PRDT + 4..PRDT + 8].copy_from_slice(&((buf_phys >> 32) as u32).to_le_bytes());
        let dbc = (buf.len() as u32 - 1) & 0x3F_FFFF; // байт минус 1, старший бит (IRQ) не ставим
        t[PRDT + 12..PRDT + 16].copy_from_slice(&dbc.to_le_bytes());

        let table_phys = phys(self.table.0.as_ptr() as u64, self.hhdm);
        let cl = &mut self.cmdlist.0;
        cl.fill(0);
        cl[0] = 5; // CFL = 5 DWORD (20-байтный H2D FIS), бит W(запись) = 0
        cl[2] = 1; // PRDTL = 1
        cl[8..12].copy_from_slice(&(table_phys as u32).to_le_bytes());
        cl[12..16].copy_from_slice(&((table_phys >> 32) as u32).to_le_bytes());

        fence(Ordering::SeqCst);
        w32(port + P_CI, 1); // выдать команду в слот 0

        for _ in 0..10_000_000u32 {
            if r32(port + P_CI) & 1 == 0 {
                return true; // контроллер снял бит — команда выполнена
            }
            if r32(port + P_IS) & IS_TFES != 0 {
                return false; // ошибка в task file
            }
        }
        false // не дождались завершения (диск завис или отключился)
    }
}
