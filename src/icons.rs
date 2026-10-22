//! Иконки интерфейса (пока только логотип на кнопке «Пуск»).
//! Формат тот же .ico, что и у курсоров, поэтому переиспользуем cursor::parse.
use crate::cursor::{parse, Cursor};
use crate::racy::Racy;

static START: Racy<Cursor> = Racy::new(Cursor::EMPTY);

pub fn load_all() {
    if let Some(c) = parse(include_bytes!("../assets/icons/start.ico")) {
        unsafe { *START.get() = c };
    }
}

pub fn start() -> &'static Cursor {
    unsafe { &*START.get() }
}
