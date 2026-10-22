//! Разбор WAV (RIFF/WAVE) — только заголовок, без декодирования звука.
//! Для музыки в разных форматах нужен полноценный кодек, а для системных
//! звуков хватает голого PCM. Наш AC97-драйвер без расширения VRA умеет
//! только 48000 Гц, поэтому файлы должны быть заранее в этом формате:
//! 16 бит, стерео, 48000 Гц. Другое — отклоняем, а не пытаемся сыграть криво.
//! (Готовые файлы конвертируются один раз при подготовке диска, см. Makefile.)

/// Вернуть срез PCM-данных (16 бит LE, стерео, 48000 Гц) внутри файла,
/// либо `None`, если это не WAV или формат не тот.
pub fn extract_pcm(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut fmt_ok = false;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if body.checked_add(size)? > data.len() {
            break; // повреждённый или обрезанный файл
        }
        if id == b"fmt " {
            if size < 16 {
                return None;
            }
            let fmt = &data[body..body + 16];
            let tag = u16::from_le_bytes([fmt[0], fmt[1]]);
            let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
            fmt_ok = tag == 1 && channels == 2 && rate == 48000 && bits == 16;
        } else if id == b"data" {
            return if fmt_ok { Some(&data[body..body + size]) } else { None };
        }
        pos = body + size + (size & 1); // чанки RIFF выровнены по чётной границе
    }
    None
}
