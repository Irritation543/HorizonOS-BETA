//! Минимальная реализация протокола загрузки Limine (base revision 2:
//! в ней вся физическая память до 4 ГиБ, включая APIC, доступна через HHDM).
//! Запросы: framebuffer, HHDM, карта памяти. Спецификация: PROTOCOL.md в репозитории Limine.
use core::cell::UnsafeCell;
use core::ptr;

const MAGIC_0: u64 = 0xc7b1dd30df4c8b88;
const MAGIC_1: u64 = 0x0a82e883a194f07b;

/// Метка версии протокола. Если загрузчик поддерживает версию,
/// он записывает 0 в последнее слово.
#[repr(C)]
pub struct BaseRevision {
    tag: [u64; 2],
    revision: UnsafeCell<u64>,
}
unsafe impl Sync for BaseRevision {}

impl BaseRevision {
    pub const fn new(revision: u64) -> Self {
        Self {
            tag: [0xf9562b2d5c95a6c8, 0x6a7b384944536bdc],
            revision: UnsafeCell::new(revision),
        }
    }

    pub fn is_supported(&self) -> bool {
        unsafe { ptr::read_volatile(self.revision.get()) == 0 }
    }
}

#[repr(C)]
pub struct RawFramebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
    pub unused: [u8; 7],
    pub edid_size: u64,
    pub edid: *mut u8,
}

#[repr(C)]
pub struct FramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: *const *const RawFramebuffer,
}

#[repr(C)]
pub struct FramebufferRequest {
    id: [u64; 4],
    revision: u64,
    response: UnsafeCell<*const FramebufferResponse>,
}
unsafe impl Sync for FramebufferRequest {}

impl FramebufferRequest {
    pub const fn new() -> Self {
        Self {
            id: [MAGIC_0, MAGIC_1, 0x9d5827dcd881dd75, 0xa3148604f6fab11b],
            revision: 0,
            response: UnsafeCell::new(ptr::null()),
        }
    }

    /// Первый framebuffer, если загрузчик его выдал.
    pub fn first(&self) -> Option<&RawFramebuffer> {
        let resp = unsafe { ptr::read_volatile(self.response.get()) };
        if resp.is_null() {
            return None;
        }
        let resp = unsafe { &*resp };
        if resp.framebuffer_count == 0 || resp.framebuffers.is_null() {
            return None;
        }
        let fb = unsafe { *resp.framebuffers };
        if fb.is_null() {
            None
        } else {
            Some(unsafe { &*fb })
        }
    }
}

// ---------------------------------------------------------------- HHDM

#[repr(C)]
pub struct HhdmResponse {
    pub revision: u64,
    pub offset: u64,
}

#[repr(C)]
pub struct HhdmRequest {
    id: [u64; 4],
    revision: u64,
    response: UnsafeCell<*const HhdmResponse>,
}
unsafe impl Sync for HhdmRequest {}

impl HhdmRequest {
    pub const fn new() -> Self {
        Self {
            id: [MAGIC_0, MAGIC_1, 0x48dcf1cb8ad2b852, 0x63984e959a98244b],
            revision: 0,
            response: UnsafeCell::new(ptr::null()),
        }
    }

    /// Смещение, с которым вся физическая память отображена в верхней половине адресов.
    pub fn offset(&self) -> Option<u64> {
        let resp = unsafe { ptr::read_volatile(self.response.get()) };
        if resp.is_null() {
            None
        } else {
            Some(unsafe { (*resp).offset })
        }
    }
}

// ---------------------------------------------------------------- карта памяти

/// Тип записи «свободная память».
const MEMMAP_USABLE: u64 = 0;

#[repr(C)]
pub struct MemmapEntry {
    pub base: u64,
    pub length: u64,
    pub kind: u64,
}

#[repr(C)]
pub struct MemmapResponse {
    pub revision: u64,
    pub entry_count: u64,
    pub entries: *const *const MemmapEntry,
}

#[repr(C)]
pub struct MemmapRequest {
    id: [u64; 4],
    revision: u64,
    response: UnsafeCell<*const MemmapResponse>,
}
unsafe impl Sync for MemmapRequest {}

impl MemmapRequest {
    pub const fn new() -> Self {
        Self {
            id: [MAGIC_0, MAGIC_1, 0x67cf3d9d378a806f, 0xe304acdfc50c3c62],
            revision: 0,
            response: UnsafeCell::new(ptr::null()),
        }
    }

    /// Самая большая область свободной памяти: (физический адрес, размер).
    pub fn largest_usable(&self) -> Option<(u64, u64)> {
        let resp = unsafe { ptr::read_volatile(self.response.get()) };
        if resp.is_null() {
            return None;
        }
        let resp = unsafe { &*resp };
        let mut best: Option<(u64, u64)> = None;
        for i in 0..resp.entry_count as usize {
            let e = unsafe { &**resp.entries.add(i) };
            if e.kind == MEMMAP_USABLE && best.map_or(true, |(_, len)| e.length > len) {
                best = Some((e.base, e.length));
            }
        }
        best
    }
}
