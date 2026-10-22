use crate::ps2::Event;
use crate::racy::Racy;
use core::sync::atomic::{AtomicUsize, Ordering};

const CAP: usize = 128;

static BUF: Racy<[Option<Event>; CAP]> = Racy::new([None; CAP]);
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);

pub fn push(ev: Event) {
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % CAP;
    if next == TAIL.load(Ordering::Acquire) {
        return;
    }
    unsafe { (*BUF.get())[head] = Some(ev) };
    HEAD.store(next, Ordering::Release);
}

pub fn pop() -> Option<Event> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let ev = unsafe { (*BUF.get())[tail] };
    TAIL.store((tail + 1) % CAP, Ordering::Release);
    ev
}

pub fn is_empty() -> bool {
    TAIL.load(Ordering::Relaxed) == HEAD.load(Ordering::Acquire)
}
