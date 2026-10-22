//! Куча ядра: подключает `alloc` (Box, Vec, String).
//! Память берётся из самой большой свободной области, вся физическая
//! память доступна через HHDM.
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

const HEAP_SIZE: u64 = 32 * 1024 * 1024;
const MIN_HEAP: u64 = 8 * 1024 * 1024;

pub fn init(hhdm: u64, region_base: u64, region_len: u64) {
    let size = region_len.min(HEAP_SIZE) & !0xFFF;
    if size < MIN_HEAP {
        crate::bsod::show(format_args!("Мало памяти: нужно минимум 8 МБ свободной области."));
    }
    unsafe { ALLOCATOR.lock().init((hhdm + region_base) as *mut u8, size as usize) };
}
