//! Входные точки прерываний (на ассемблере).
//! Каждая заглушка кладёт на стек код ошибки (или 0) и номер вектора,
//! сохраняет регистры и вызывает `interrupt_dispatch(&mut Frame)`.
use core::arch::global_asm;

global_asm!(
    r#"
.macro isr_noerr n
isr_stub_\n:
    push 0
    push \n
    jmp isr_common
.endm

.macro isr_err n
isr_stub_\n:
    push \n
    jmp isr_common
.endm

isr_common:
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    cld
    call interrupt_dispatch
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    pop rdx
    pop rcx
    pop rax
    add rsp, 16
    iretq

    isr_noerr 0
    isr_noerr 1
    isr_noerr 2
    isr_noerr 3
    isr_noerr 4
    isr_noerr 5
    isr_noerr 6
    isr_noerr 7
    isr_err 8
    isr_noerr 9
    isr_err 10
    isr_err 11
    isr_err 12
    isr_err 13
    isr_err 14
    isr_noerr 15
    isr_noerr 16
    isr_err 17
    isr_noerr 18
    isr_noerr 19
    isr_noerr 20
    isr_err 21
    isr_noerr 22
    isr_noerr 23
    isr_noerr 24
    isr_noerr 25
    isr_noerr 26
    isr_noerr 27
    isr_noerr 28
    isr_err 29
    isr_err 30
    isr_noerr 31

    /* IRQ: keyboard 0x21, RTC 0x28, mouse 0x2C, spurious 0xFF */
    isr_noerr 33
    isr_noerr 40
    isr_noerr 44
    isr_noerr 255

    .pushsection .rodata
    .balign 8
    .global ISR_TABLE
ISR_TABLE:
    .quad isr_stub_0
    .quad isr_stub_1
    .quad isr_stub_2
    .quad isr_stub_3
    .quad isr_stub_4
    .quad isr_stub_5
    .quad isr_stub_6
    .quad isr_stub_7
    .quad isr_stub_8
    .quad isr_stub_9
    .quad isr_stub_10
    .quad isr_stub_11
    .quad isr_stub_12
    .quad isr_stub_13
    .quad isr_stub_14
    .quad isr_stub_15
    .quad isr_stub_16
    .quad isr_stub_17
    .quad isr_stub_18
    .quad isr_stub_19
    .quad isr_stub_20
    .quad isr_stub_21
    .quad isr_stub_22
    .quad isr_stub_23
    .quad isr_stub_24
    .quad isr_stub_25
    .quad isr_stub_26
    .quad isr_stub_27
    .quad isr_stub_28
    .quad isr_stub_29
    .quad isr_stub_30
    .quad isr_stub_31
    .global IRQ_TABLE
IRQ_TABLE:
    .quad isr_stub_33
    .quad isr_stub_40
    .quad isr_stub_44
    .quad isr_stub_255
    .popsection
"#
);
