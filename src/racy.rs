//! Глобальные данные, которые мы сами защищаем от гонок
//! (одно ядро CPU, обработчики прерываний не вложены).
use core::cell::UnsafeCell;

pub struct Racy<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Racy<T> {}

impl<T> Racy<T> {
    pub const fn new(value: T) -> Self {
        Racy(UnsafeCell::new(value))
    }

    pub fn get(&self) -> *mut T {
        self.0.get()
    }
}
