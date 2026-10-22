use crate::pci;
use crate::port::{inl, inw, outb, outl, outw};
use alloc::boxed::Box;
use alloc::vec;

const NAM_MASTER_VOL: u16 = 0x02;
const NAM_PCM_VOL: u16 = 0x18;

const PO_BDBAR: u16 = 0x10;
const PO_LVI: u16 = 0x15;
const PO_SR: u16 = 0x16;
const PO_CR: u16 = 0x1B;
const GLOB_CNT: u16 = 0x2C;
const GLOB_STA: u16 = 0x30;

const CR_RPBM: u8 = 1 << 0;
const SR_DCH: u16 = 1 << 0;
const GLOB_STA_PCR: u32 = 1 << 8;

const BDL_ENTRIES: usize = 32;
const MAX_FRAMES_PER_ENTRY: u32 = 0xFFFE;
const BYTES_PER_FRAME: usize = 4;

#[repr(C, align(8))]
struct Bdl([u8; BDL_ENTRIES * 8]);

pub struct Ac97 {
    nabm: u16,
    hhdm: u64,
    bdl: Box<Bdl>,
    current: Option<alloc::vec::Vec<u32>>,
}

fn phys(virt_addr: u64, hhdm: u64) -> u64 {
    virt_addr - hhdm
}

pub fn init(hhdm: u64) -> Option<Ac97> {
    let dev = pci::find(0x04, 0x01, 0x00)?;
    dev.enable_bus_master();

    let bar0 = dev.bar(0);
    let bar1 = dev.bar(1);
    if bar0 & 0x1 == 0 || bar1 & 0x1 == 0 {
        return None;
    }
    let nam = (bar0 & 0xFFFC) as u16;
    let nabm = (bar1 & 0xFFFC) as u16;

    unsafe { outl(nabm + GLOB_CNT, 0x0000_0002) };

    let mut ready = false;
    for _ in 0..1_000_000 {
        if unsafe { inl(nabm + GLOB_STA) } & GLOB_STA_PCR != 0 {
            ready = true;
            break;
        }
    }
    if !ready {
        return None;
    }

    unsafe {
        outw(nam + NAM_MASTER_VOL, 0x0000);
        outw(nam + NAM_PCM_VOL, 0x0000);
    }

    Some(Ac97 { nabm, hhdm, bdl: Box::new(Bdl([0; BDL_ENTRIES * 8])), current: None })
}

impl Ac97 {
    fn start_inner(&mut self, pcm: &[u8]) -> Option<u32> {
        if pcm.len() < BYTES_PER_FRAME {
            return Some(0);
        }

        unsafe { outb(self.nabm + PO_CR, 0) };
        for _ in 0..1_000_000 {
            if unsafe { inw(self.nabm + PO_SR) } & SR_DCH != 0 {
                break;
            }
        }

        let words = (pcm.len() + 3) / 4;
        let mut aligned = vec![0u32; words];
        unsafe {
            core::ptr::copy_nonoverlapping(pcm.as_ptr(), aligned.as_mut_ptr() as *mut u8, pcm.len());
        }
        let total_frames = (pcm.len() / BYTES_PER_FRAME) as u32;
        let base_phys = phys(aligned.as_ptr() as u64, self.hhdm);

        let mut entries = 0usize;
        let mut done_frames = 0u32;
        while done_frames < total_frames && entries < BDL_ENTRIES {
            let n = (total_frames - done_frames).min(MAX_FRAMES_PER_ENTRY);
            let last = done_frames + n >= total_frames || entries + 1 == BDL_ENTRIES;
            let addr = base_phys + done_frames as u64 * BYTES_PER_FRAME as u64;

            let mut ctrl: u16 = 0;
            if last {
                ctrl |= 1 << 14;
            }

            let e = entries * 8;
            self.bdl.0[e..e + 4].copy_from_slice(&(addr as u32).to_le_bytes());
            self.bdl.0[e + 4..e + 6].copy_from_slice(&(n as u16).to_le_bytes());
            self.bdl.0[e + 6..e + 8].copy_from_slice(&ctrl.to_le_bytes());

            done_frames += n;
            entries += 1;
        }
        if entries == 0 {
            return None;
        }
        let last_valid = (entries - 1) as u8;

        self.current = Some(aligned);

        let bdl_phys = phys(self.bdl.0.as_ptr() as u64, self.hhdm);
        unsafe {
            outl(self.nabm + PO_BDBAR, bdl_phys as u32);
            outb(self.nabm + PO_LVI, last_valid);
            outb(self.nabm + PO_CR, CR_RPBM);
        }
        Some(total_frames)
    }

    pub fn start(&mut self, pcm: &[u8]) -> bool {
        self.start_inner(pcm).is_some()
    }

    pub fn is_playing(&self) -> bool {
        let sr = unsafe { inw(self.nabm + PO_SR) };
        sr & SR_DCH == 0 && self.current.is_some()
    }

    pub fn service(&mut self) {
        if self.current.is_none() {
            return;
        }
        let sr = unsafe { inw(self.nabm + PO_SR) };
        if sr & SR_DCH != 0 {
            self.current = None;
        }
    }

    pub fn play_blocking(&mut self, pcm: &[u8]) -> bool {
        let Some(total_frames) = self.start_inner(pcm) else {
            return false;
        };
        if total_frames == 0 {
            return true;
        }

        let expected_secs = (total_frames + 47_999) / 48_000 + 1;
        crate::rtc::take_tick();
        let mut elapsed_secs = 0u32;
        'wait: loop {
            for _ in 0..50_000 {
                if unsafe { inw(self.nabm + PO_SR) } & SR_DCH != 0 {
                    break 'wait;
                }
            }
            if crate::rtc::take_tick() {
                elapsed_secs += 1;
            }
            if elapsed_secs >= expected_secs {
                break;
            }
        }
        unsafe { outb(self.nabm + PO_CR, 0) };
        true
    }
}
