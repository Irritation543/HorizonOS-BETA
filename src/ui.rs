//! Рабочий стол, панель задач, кнопка «Пуск», меню, часы, индикатор раскладки.
use crate::gfx::{Screen, DARK, DESKTOP, FACE, LIGHT, NAVY, SHADOW, WHITE};
use crate::rtc::Time;
use alloc::string::String;

pub const TASKBAR_H: i32 = 28;
const START_W: i32 = 64;
const START_H: i32 = 22;
const MENU_W: i32 = 190;
const MENU_ITEM_H: i32 = 24;
const MENU_ITEMS: [&str; 3] = ["Программы", "Перезагрузка", "Завершение работы"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Reboot,
    Shutdown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    En,
    Ru,
}

/// Какой элемент модального окна сейчас нажат мышью (для отклика при
/// отпускании кнопки ровно там же, где было нажатие).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModalCtrl {
    None,
    Close,
    Shutdown,
    Reboot,
}

/// Модальные окна поверх всего интерфейса: пока только диалог питания.
/// Пока открыто — остальной интерфейс не реагирует на ввод (см. mouse/key).
enum Modal {
    PcManager { pressed: ModalCtrl },
}

const MODAL_W: i32 = 340;
const MODAL_H: i32 = 78;
const MODAL_TITLE_H: i32 = 20;
const MODAL_BTN_W: i32 = 100;
const MODAL_BTN_H: i32 = 22;
const MODAL_BTN_GAP: i32 = 8;

pub struct Ui {
    w: i32,
    h: i32,
    pub time: Time,
    /// Временная строка для проверки диска A: (шаг с FAT32). Уйдёт, когда
    /// появится настоящий рабочий стол со значками.
    pub disk_status: Option<String>,
    modal: Option<Modal>,
    layout: Layout,
    menu_open: bool,
    start_down: bool,
    hover: Option<usize>,
    shift: bool,
    ctrl: bool,
    alt: bool,
}

impl Ui {
    pub fn new(w: i32, h: i32) -> Ui {
        Ui {
            w,
            h,
            time: Time { h: 0, m: 0 },
            disk_status: None,
            modal: None,
            layout: Layout::En,
            menu_open: false,
            start_down: false,
            hover: None,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    fn taskbar_y(&self) -> i32 {
        self.h - TASKBAR_H
    }

    fn start_rect(&self) -> (i32, i32, i32, i32) {
        (2, self.taskbar_y() + 3, START_W, START_H)
    }

    fn in_start(&self, x: i32, y: i32) -> bool {
        let (bx, by, bw, bh) = self.start_rect();
        x >= bx && x < bx + bw && y >= by && y < by + bh
    }

    fn menu_rect(&self) -> (i32, i32, i32, i32) {
        let h = 4 + MENU_ITEMS.len() as i32 * MENU_ITEM_H;
        (2, self.taskbar_y() - h, MENU_W, h)
    }

    fn menu_item_at(&self, x: i32, y: i32) -> Option<usize> {
        let (mx, my, mw, mh) = self.menu_rect();
        if x >= mx + 2 && x < mx + mw - 2 && y >= my + 2 && y < my + mh - 2 {
            Some(((y - my - 2) / MENU_ITEM_H) as usize)
        } else {
            None
        }
    }

    /// Прямоугольник самого модального окна (по центру экрана).
    fn modal_rect(&self) -> (i32, i32, i32, i32) {
        ((self.w - MODAL_W) / 2, (self.h - MODAL_H) / 2, MODAL_W, MODAL_H)
    }

    /// Кнопка закрытия [x] в заголовке модального окна.
    fn modal_close_rect(&self) -> (i32, i32, i32, i32) {
        let (x, y, w, _) = self.modal_rect();
        (x + w - 22, y + 2, 18, 16)
    }

    /// Кнопки «Завершение работы» / «Перезагрузка» / «Спящий режим» (последняя неактивна).
    fn modal_button_rects(&self) -> [(i32, i32, i32, i32); 3] {
        let (x, y, w, h) = self.modal_rect();
        let total = MODAL_BTN_W * 3 + MODAL_BTN_GAP * 2;
        let start_x = x + (w - total) / 2;
        let by = y + MODAL_TITLE_H + (h - MODAL_TITLE_H - MODAL_BTN_H) / 2;
        core::array::from_fn(|i| {
            (start_x + i as i32 * (MODAL_BTN_W + MODAL_BTN_GAP), by, MODAL_BTN_W, MODAL_BTN_H)
        })
    }

    fn modal_hit(&self, x: i32, y: i32) -> ModalCtrl {
        let in_rect = |px: i32, py: i32, r: (i32, i32, i32, i32)| {
            px >= r.0 && px < r.0 + r.2 && py >= r.1 && py < r.1 + r.3
        };
        if in_rect(x, y, self.modal_close_rect()) {
            return ModalCtrl::Close;
        }
        let [shutdown, reboot, _sleep] = self.modal_button_rects();
        if in_rect(x, y, shutdown) {
            return ModalCtrl::Shutdown;
        }
        if in_rect(x, y, reboot) {
            return ModalCtrl::Reboot;
        }
        ModalCtrl::None // включая «Спящий режим» — она намеренно не реагирует
    }

    /// Ввод, пока открыто модальное окно: остальной интерфейс не трогаем.
    fn modal_mouse(&mut self, x: i32, y: i32, buttons: u8, prev: u8) -> (bool, Action) {
        let down = buttons & 1 != 0 && prev & 1 == 0;
        let up = buttons & 1 == 0 && prev & 1 != 0;
        let hit = self.modal_hit(x, y);
        let mut dirty = false;
        let mut action = Action::None;

        let Some(Modal::PcManager { pressed }) = &mut self.modal else {
            return (false, Action::None);
        };

        if down && hit != ModalCtrl::None {
            *pressed = hit;
            dirty = true;
        }
        if up {
            let was = core::mem::replace(pressed, ModalCtrl::None);
            if was != ModalCtrl::None {
                dirty = true;
            }
            if was != ModalCtrl::None && was == hit {
                match was {
                    ModalCtrl::Close => self.modal = None,
                    ModalCtrl::Shutdown => {
                        self.modal = None;
                        action = Action::Shutdown;
                    }
                    ModalCtrl::Reboot => {
                        self.modal = None;
                        action = Action::Reboot;
                    }
                    ModalCtrl::None => {}
                }
            }
        }
        (dirty, action)
    }

    /// В меню «Пуск»: «Программы» пока не делает ничего, питание — открывает диалог.
    fn activate(&mut self, i: usize) {
        if i == 1 || i == 2 {
            self.modal = Some(Modal::PcManager { pressed: ModalCtrl::None });
        }
    }

    // ---------------------------------------------------------- ввод

    /// Вернёт (нужна перерисовка, действие).
    pub fn mouse(&mut self, x: i32, y: i32, buttons: u8, prev: u8) -> (bool, Action) {
        if self.modal.is_some() {
            return self.modal_mouse(x, y, buttons, prev);
        }

        let mut dirty = false;
        let action = Action::None;

        if self.menu_open {
            let h = self.menu_item_at(x, y);
            if h != self.hover {
                self.hover = h;
                dirty = true;
            }
        }

        let down = buttons & 1 != 0 && prev & 1 == 0;
        let up = buttons & 1 == 0 && prev & 1 != 0;

        if down {
            if self.in_start(x, y) {
                self.start_down = true;
                dirty = true;
            } else if self.menu_open && self.menu_item_at(x, y).is_none() {
                self.menu_open = false;
                self.hover = None;
                dirty = true;
            }
        }

        if up {
            if self.start_down {
                self.start_down = false;
                if self.in_start(x, y) {
                    self.menu_open = !self.menu_open;
                    self.hover = None;
                }
                dirty = true;
            } else if self.menu_open {
                if let Some(i) = self.menu_item_at(x, y) {
                    self.activate(i);
                    self.menu_open = false;
                    self.hover = None;
                    dirty = true;
                }
            }
        }
        (dirty, action)
    }

    pub fn key(&mut self, code: u8, ext: bool, pressed: bool) -> (bool, Action) {
        if self.modal.is_some() {
            return (false, Action::None); // клавиатура диалог пока не трогает — только мышь
        }

        let is_shift = !ext && (code == 0x2A || code == 0x36);
        let is_ctrl = code == 0x1D;
        let is_alt = code == 0x38;

        // «свежее» нажатие модификатора (без учёта автоповтора)
        let mut fresh = false;
        if is_shift {
            fresh = pressed && !self.shift;
            self.shift = pressed;
        }
        if is_ctrl {
            fresh = pressed && !self.ctrl;
            self.ctrl = pressed;
        }
        if is_alt {
            fresh = pressed && !self.alt;
            self.alt = pressed;
        }

        let mut dirty = false;
        let action = Action::None;
        if !pressed {
            return (false, action);
        }

        // Alt+Shift или Ctrl+Shift — смена раскладки EN/RU
        if fresh && ((is_shift && (self.alt || self.ctrl)) || ((is_alt || is_ctrl) && self.shift)) {
            self.layout = if self.layout == Layout::En { Layout::Ru } else { Layout::En };
            dirty = true;
        }

        match (ext, code) {
            // Esc закрывает меню; Ctrl+Esc открывает его, как в Windows
            (false, 0x01) => {
                self.menu_open = if self.ctrl { !self.menu_open } else { false };
                self.hover = None;
                dirty = true;
            }
            // клавиша Win (левая/правая)
            (true, 0x5B) | (true, 0x5C) => {
                self.menu_open = !self.menu_open;
                self.hover = None;
                dirty = true;
            }
            // стрелка вверх
            (true, 0x48) if self.menu_open => {
                self.hover = Some(match self.hover {
                    Some(i) if i > 0 => i - 1,
                    _ => MENU_ITEMS.len() - 1,
                });
                dirty = true;
            }
            // стрелка вниз
            (true, 0x50) if self.menu_open => {
                self.hover = Some(match self.hover {
                    Some(i) if i + 1 < MENU_ITEMS.len() => i + 1,
                    _ => 0,
                });
                dirty = true;
            }
            // Enter
            (false, 0x1C) if self.menu_open => {
                if let Some(i) = self.hover {
                    self.activate(i);
                    self.menu_open = false;
                    self.hover = None;
                    dirty = true;
                }
            }
            _ => {}
        }
        (dirty, action)
    }

    // ---------------------------------------------------------- рисование

    pub fn draw(&self, s: &mut Screen) {
        // рабочий стол
        s.fill_rect(0, 0, self.w, self.h, DESKTOP);
        if let Some(status) = &self.disk_status {
            s.text(8, 8, status, WHITE);
        }

        // панель задач
        let ty = self.taskbar_y();
        s.fill_rect(0, ty, self.w, TASKBAR_H, FACE);
        s.fill_rect(0, ty, self.w, 1, LIGHT);
        s.fill_rect(0, ty + 1, self.w, 1, WHITE);

        // кнопка «Пуск»
        let (bx, by, bw, bh) = self.start_rect();
        let pressed = self.start_down || self.menu_open;
        if pressed {
            s.sunken2(bx, by, bw, bh);
        } else {
            s.raised2(bx, by, bw, bh);
        }
        let o = if pressed { 1 } else { 0 };
        let icon = crate::icons::start();
        s.blit_icon(icon, bx + 5 + o, by + (bh - icon.h as i32) / 2 + o);
        s.text_bold(bx + 5 + icon.w as i32 + 4 + o, by + 3 + o, "Пуск", DARK);

        // разделитель
        s.fill_rect(bx + bw + 4, by, 1, bh, SHADOW);
        s.fill_rect(bx + bw + 5, by, 1, bh, WHITE);

        // трей: раскладка и часы
        let tw = 96;
        let tx = self.w - tw - 2;
        s.sunken1(tx, by, tw, bh);
        let lang = match self.layout {
            Layout::En => "EN",
            Layout::Ru => "RU",
        };
        s.text(tx + 8, by + 3, lang, DARK);
        let t = self.time;
        let buf = [b'0' + t.h / 10, b'0' + t.h % 10, b':', b'0' + t.m / 10, b'0' + t.m % 10];
        let clock = core::str::from_utf8(&buf).unwrap_or("--:--");
        s.text(tx + tw - 8 - 5 * crate::gfx::CHAR_W, by + 3, clock, DARK);

        // меню «Пуск»
        if self.menu_open {
            let (mx, my, mw, mh) = self.menu_rect();
            s.raised2(mx, my, mw, mh);
            for (i, label) in MENU_ITEMS.iter().enumerate() {
                let iy = my + 2 + i as i32 * MENU_ITEM_H;
                let selected = self.hover == Some(i);
                if selected {
                    s.fill_rect(mx + 2, iy, mw - 4, MENU_ITEM_H, NAVY);
                }
                let color = if selected { WHITE } else { DARK };
                s.text(mx + 12, iy + (MENU_ITEM_H - 16) / 2, label, color);
            }
        }

        self.draw_modal(s);
    }

    fn draw_modal(&self, s: &mut Screen) {
        let Some(Modal::PcManager { pressed }) = &self.modal else {
            return;
        };

        s.dim_all(140); // весь интерфейс позади становится серым и недоступным

        let (x, y, w, h) = self.modal_rect();
        s.raised2(x, y, w, h);

        // заголовок
        s.fill_rect(x + 2, y + 2, w - 4, MODAL_TITLE_H - 2, NAVY);
        s.text_bold(x + 8, y + 2 + (MODAL_TITLE_H - 2 - 16) / 2, "PC Manager", WHITE);

        let (cx, cy, cw, ch) = self.modal_close_rect();
        if *pressed == ModalCtrl::Close {
            s.sunken2(cx, cy, cw, ch);
        } else {
            s.raised2(cx, cy, cw, ch);
        }
        let co = if *pressed == ModalCtrl::Close { 1 } else { 0 };
        s.text(cx + (cw - 8) / 2 + co, cy + co, "x", DARK);

        // кнопки
        let [shutdown, reboot, sleep] = self.modal_button_rects();
        for (rect, label, ctrl, enabled) in [
            (shutdown, "Shutdown", ModalCtrl::Shutdown, true),
            (reboot, "Reboot", ModalCtrl::Reboot, true),
            (sleep, "Sleep mode", ModalCtrl::None, false),
        ] {
            let (bx, by, bw, bh) = rect;
            let down = enabled && *pressed == ctrl;
            if down {
                s.sunken2(bx, by, bw, bh);
            } else {
                s.raised2(bx, by, bw, bh);
            }
            let o = if down { 1 } else { 0 };
            let tx = bx + (bw - label.len() as i32 * crate::gfx::CHAR_W) / 2 + o;
            let ty = by + (bh - 16) / 2 + o;
            if enabled {
                s.text(tx, ty, label, DARK);
            } else {
                s.text_disabled(tx, ty, label);
            }
        }
    }
}
