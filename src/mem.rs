//! memcpy/memset/memmove/memcmp для no_std.
//! Компилятор генерирует вызовы этих функций сам (копирование структур, fill и т.д.).
//! Реализованы через `rep movsb/stosb`, чтобы LLVM не превратил тело функции
//! в рекурсивный вызов самой себя.
//!
//! `#[allow(...)]` ниже: новый lint компилятора сверяет сигнатуру с Си (ждёт
//! `*mut c_void` вместо `*mut u8`), это только типизация, поведение то же.
#![allow(suspicious_runtime_symbol_definitions)]
use core::arch::asm;

#[no_mangle]
pub unsafe extern "C" fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    asm!(
        "rep movsb",
        inout("rcx") n => _,
        inout("rdi") dst => _,
        inout("rsi") src => _,
        options(nostack, preserves_flags)
    );
    dst
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if n == 0 {
        return dst;
    }
    let d = dst as usize;
    let s = src as usize;
    if d <= s || d >= s + n {
        memcpy(dst, src, n);
    } else {
        // области пересекаются и dst позже src: копируем с конца
        asm!(
            "std",
            "rep movsb",
            "cld",
            inout("rcx") n => _,
            inout("rdi") dst.add(n - 1) => _,
            inout("rsi") src.add(n - 1) => _,
            options(nostack)
        );
    }
    dst
}

#[no_mangle]
pub unsafe extern "C" fn memset(dst: *mut u8, value: i32, n: usize) -> *mut u8 {
    asm!(
        "rep stosb",
        inout("rcx") n => _,
        inout("rdi") dst => _,
        in("al") value as u8,
        options(nostack, preserves_flags)
    );
    dst
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let x = *a.add(i);
        let y = *b.add(i);
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    memcmp(a, b, n)
}
