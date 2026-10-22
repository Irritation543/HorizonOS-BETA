#![no_std]
#![no_main]

extern crate alloc;

mod ac97;
mod ahci;
mod apic;
mod bsod;
mod cursor;
mod fat32;
mod events;
mod font_data;
mod gdt;
mod gfx;
mod heap;
mod icons;
mod idt;
mod isr;
mod limine;
mod mem;
mod pci;
mod port;
mod power;
mod ps2;
mod racy;
mod rtc;
mod ui;
mod wav;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use core::arch::asm;
use core::panic::PanicInfo;
use limine::{BaseRevision, FramebufferRequest, HhdmRequest, MemmapRequest};

// Запросы к загрузчику Limine (секция .requests сохраняется линкером, см. linker.ld)
#[used]
#[link_section = ".requests"]
static BASE_REVISION: BaseRevision = BaseRevision::new(2);

#[used]
#[link_section = ".requests"]
static FRAMEBUFFER: FramebufferRequest = FramebufferRequest::new();

#[used]
#[link_section = ".requests"]
static HHDM: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".requests"]
static MEMMAP: MemmapRequest = MemmapRequest::new();

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    if !BASE_REVISION.is_supported() {
        power::halt();
    }

    // Сначала таблицы прерываний: любая ошибка дальше покажется синим экраном.
    gdt::init();
    idt::init();

    let fb = match FRAMEBUFFER.first() {
        Some(f) => f,
        None => power::halt(),
    };
    let hhdm = match HHDM.offset() {
        Some(o) => o,
        None => bsod::show(format_args!("Загрузчик не выдал карту физической памяти (HHDM).")),
    };
    let (region_base, region_len) = match MEMMAP.largest_usable() {
        Some(r) => r,
        None => bsod::show(format_args!("Загрузчик не выдал карту памяти.")),
    };
    heap::init(hhdm, region_base, region_len);

    // задний буфер точно по размеру экрана
    let pixels = fb.width as usize * fb.height as usize;
    let back = Box::leak(vec![0u32; pixels].into_boxed_slice()).as_mut_ptr();
    let mut screen = match gfx::Screen::new(fb, back) {
        Some(s) => s,
        None => bsod::show(format_args!("Неподдерживаемый видеорежим: нужно 32 бита на пиксель.")),
    };

    // Экран загрузки — сразу, ещё до долгой части (диск, звук). Курсора
    // здесь нет: мышь ещё не инициализирована, да и не нужна на этом этапе.
    screen.blit_rgb888(include_bytes!("../assets/boot/splash.bin"));
    screen.present_all();

    cursor::load_all();
    icons::load_all();
    apic::init(hhdm);
    ps2::init();
    rtc::enable_tick();
    unsafe { asm!("sti", options(nomem, nostack)) }; // нужно уже сейчас: ac97 ждёт звук по тикам RTC

    let mut ui = ui::Ui::new(screen.w, screen.h);
    ui.time = rtc::read();

    // Диск A: и FAT32. Строка на рабочем столе — временная проверка для
    // этого шага, уйдёт, когда появится нормальный рабочий стол с иконками.
    let mut disk = ahci::init(hhdm);
    let mut fs = None;
    let mut status = String::from("Диск A: не найден");
    if let Some(d) = disk.as_mut() {
        status = match fat32::mount(d) {
            Some(f) => {
                let n = f.list_root(d).len();
                fs = Some(f);
                format!("Диск A: {n} файлов в корне")
            }
            None => String::from("Диск A: подключён, но не отформатирован в FAT32"),
        };
    }
    ui.disk_status = Some(status);

    // Звук (AC97). Startup/shutdown читаем с диска один раз здесь: при
    // выключении диск уже может быть не нужен, звук проще держать в памяти.
    let mut ac97 = ac97::init(hhdm);
    let mut startup_wav = None;
    let mut shutdown_wav = None;
    if let (Some(d), Some(f)) = (disk.as_mut(), fs.as_ref()) {
        for e in f.list_root(d) {
            if e.matches("STARTUP.WAV") {
                startup_wav = f.read_file(d, &e);
            } else if e.matches("SHUTDOWN.WAV") {
                shutdown_wav = f.read_file(d, &e);
            }
        }
    }

    let mut mx = screen.w / 2;
    let mut my = screen.h / 2;
    let mut buttons: u8 = 0;
    let mut kind: usize = 1; // app_starting — ОС ещё не полностью готова

    // Этап 2: голый рабочий стол (фон + курсор), без такскбара. Звук
    // стартует одновременно — играет в фоне, ждать его не нужно.
    screen.fill_rect(0, 0, screen.w, screen.h, gfx::DESKTOP);
    screen.present_all();
    screen.draw_cursor(cursor::get(kind), mx, my);

    if let (Some(ac), Some(bytes)) = (ac97.as_mut(), startup_wav.as_deref()) {
        if let Some(pcm) = wav::extract_pcm(bytes) {
            ac.start(pcm); // играет в фоне — рабочий стол сразу отзывчивый
        }
    }

    // Этап 3: полный интерфейс — курсор снова обычный.
    kind = 0;
    ui.draw(&mut screen);
    screen.present_all();
    screen.draw_cursor(cursor::get(kind), mx, my);

    loop {
        let (old_x, old_y, old_kind) = (mx, my, kind);
        let mut redraw = false;
        let mut action = ui::Action::None;

        // события клавиатуры и мыши, накопленные обработчиками прерываний
        while let Some(ev) = events::pop() {
            let (dirty, act) = match ev {
                ps2::Event::Mouse { dx, dy, buttons: b } => {
                    mx = (mx + dx).clamp(0, screen.w - 1);
                    my = (my + dy).clamp(0, screen.h - 1);
                    let r = ui.mouse(mx, my, b, buttons);
                    buttons = b;
                    r
                }
                ps2::Event::Key { code, extended, pressed } => {
                    // тест курсоров: клавиши 1..8
                    if pressed && !extended && (0x02..=0x09).contains(&code) {
                        kind = (code - 0x02) as usize;
                    }
                    ui.key(code, extended, pressed)
                }
            };
            redraw |= dirty;
            if act != ui::Action::None {
                action = act;
            }
        }

        match action {
            ui::Action::Reboot => power::reboot_with_screen(&mut screen),
            ui::Action::Shutdown => {
                power::shutdown_with_screen(&mut screen, ac97.as_mut(), shutdown_wav.as_deref())
            }
            ui::Action::None => {}
        }

        // часы: раз в секунду приходит прерывание RTC
        if rtc::take_tick() {
            let t = rtc::read();
            if t != ui.time {
                ui.time = t;
                redraw = true;
            }
        }

        // если звук доиграл сам — остановить канал и освободить буфер
        if let Some(ac) = ac97.as_mut() {
            ac.service();
        }

        if redraw {
            ui.draw(&mut screen);
            screen.present_all();
            screen.draw_cursor(cursor::get(kind), mx, my);
        } else if mx != old_x || my != old_y || kind != old_kind {
            // стереть старый курсор (вернуть кусок сцены) и нарисовать новый
            let oc = cursor::get(old_kind);
            let size = cursor::MAX as i32;
            screen.present_rect(old_x - oc.hot_x, old_y - oc.hot_y, size, size);
            screen.draw_cursor(cursor::get(kind), mx, my);
        }

        // Спим до следующего прерывания. cli перед проверкой, чтобы событие
        // не пришло между проверкой и hlt (sti действует после следующей команды).
        unsafe { asm!("cli", options(nomem, nostack)) };
        if events::is_empty() && !rtc::tick_pending() {
            unsafe { asm!("sti", "hlt", options(nomem, nostack)) };
        } else {
            unsafe { asm!("sti", options(nomem, nostack)) };
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    bsod::show(format_args!("Критическая ошибка ядра:\n{}", info))
}
