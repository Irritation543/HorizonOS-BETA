//! Файловая система FAT32, только чтение. Диск размечен без таблицы
//! разделов («суперфлоппи»): FAT32 начинается прямо с LBA 0. Длинные
//! имена (VFAT/LFN) пока не разбираются — только короткие имена 8.3.
use crate::ahci::Disk;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub struct Fs {
    sectors_per_cluster: u32,
    first_fat_sector: u32,
    first_data_sector: u32,
    root_cluster: u32,
}

pub struct DirEntry {
    name: [u8; 11], // как на диске: 8 байт имени + 3 байта расширения, пробелами дополнено
    pub is_dir: bool,
    pub size: u32,
    pub cluster: u32,
}

impl DirEntry {
    /// Имя вида «ИМЯ.РАСШИРЕНИЕ» для показа пользователю.
    pub fn display_name(&self) -> String {
        let base = core::str::from_utf8(&self.name[0..8]).unwrap_or("").trim_end();
        let ext = core::str::from_utf8(&self.name[8..11]).unwrap_or("").trim_end();
        if ext.is_empty() {
            format!("{base}")
        } else {
            format!("{base}.{ext}")
        }
    }

    /// Сравнение с именем в формате 8.3 без учёта регистра, например "HELLO.TXT".
    pub fn matches(&self, short_name: &str) -> bool {
        self.display_name().eq_ignore_ascii_case(short_name)
    }
}

/// Разобрать загрузочный сектор (BPB) и подготовиться к чтению.
/// `None`, если это не FAT32 с сектором 512 байт (например, диск не
/// подключён, не отформатирован, или это FAT12/16).
pub fn mount(disk: &mut Disk) -> Option<Fs> {
    let mut boot = vec![0u8; 512]; // должен быть в куче — см. предупреждение в ahci.rs
    if !disk.read(0, 1, &mut boot) {
        return None;
    }
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return None; // нет сигнатуры загрузочного сектора
    }
    let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]) as u32;
    let sectors_per_cluster = boot[13] as u32;
    let reserved = u16::from_le_bytes([boot[14], boot[15]]) as u32;
    let num_fats = boot[16] as u32;
    let fat_size32 = u32::from_le_bytes(boot[36..40].try_into().unwrap());
    let root_cluster = u32::from_le_bytes(boot[44..48].try_into().unwrap());

    if bytes_per_sector != 512 || fat_size32 == 0 || root_cluster < 2 || sectors_per_cluster == 0 {
        return None; // не FAT32, или нестандартный размер сектора — не поддерживаем
    }

    Some(Fs {
        sectors_per_cluster,
        first_fat_sector: reserved,
        first_data_sector: reserved + num_fats * fat_size32,
        root_cluster,
    })
}

impl Fs {
    fn cluster_lba(&self, cluster: u32) -> u64 {
        (self.first_data_sector + (cluster - 2) * self.sectors_per_cluster) as u64
    }

    /// Следующий кластер в цепочке или `None`, если это был последний.
    fn next_cluster(&self, disk: &mut Disk, cluster: u32) -> Option<u32> {
        let fat_offset = cluster * 4;
        let sector = self.first_fat_sector + fat_offset / 512;
        let offset = (fat_offset % 512) as usize;
        let mut buf = vec![0u8; 512];
        if !disk.read(sector as u64, 1, &mut buf) {
            return None;
        }
        let val = u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap()) & 0x0FFF_FFFF;
        if val >= 0x0FFF_FFF8 || val == 0 {
            None
        } else {
            Some(val)
        }
    }

    fn read_dir(&self, disk: &mut Disk, first_cluster: u32) -> Vec<DirEntry> {
        let mut out = Vec::new();
        let cluster_bytes = self.sectors_per_cluster as usize * 512;
        let mut buf = vec![0u8; cluster_bytes];
        let mut cluster = first_cluster;
        loop {
            if !disk.read(self.cluster_lba(cluster), self.sectors_per_cluster, &mut buf) {
                return out;
            }
            for chunk in buf.chunks_exact(32) {
                match chunk[0] {
                    0x00 => return out, // конец каталога
                    0xE5 => continue,   // удалённая запись
                    _ => {}
                }
                let attr = chunk[11];
                if attr == 0x0F || attr & 0x08 != 0 {
                    continue; // часть длинного имени или метка тома — пропускаем
                }
                let mut name = [0u8; 11];
                name.copy_from_slice(&chunk[0..11]);
                let hi = u16::from_le_bytes([chunk[20], chunk[21]]) as u32;
                let lo = u16::from_le_bytes([chunk[26], chunk[27]]) as u32;
                let size = u32::from_le_bytes(chunk[28..32].try_into().unwrap());
                out.push(DirEntry { name, is_dir: attr & 0x10 != 0, size, cluster: (hi << 16) | lo });
            }
            match self.next_cluster(disk, cluster) {
                Some(c) => cluster = c,
                None => return out,
            }
        }
    }

    pub fn list_root(&self, disk: &mut Disk) -> Vec<DirEntry> {
        self.read_dir(disk, self.root_cluster)
    }

    /// Прочитать файл целиком по его записи в каталоге.
    pub fn read_file(&self, disk: &mut Disk, entry: &DirEntry) -> Option<Vec<u8>> {
        let mut out = vec![0u8; entry.size as usize];
        if out.is_empty() {
            return Some(out);
        }
        let cluster_bytes = self.sectors_per_cluster as usize * 512;
        let mut tmp = vec![0u8; cluster_bytes];
        let mut cluster = entry.cluster;
        let mut off = 0usize;
        while off < out.len() {
            if !disk.read(self.cluster_lba(cluster), self.sectors_per_cluster, &mut tmp) {
                return None;
            }
            let take = cluster_bytes.min(out.len() - off);
            out[off..off + take].copy_from_slice(&tmp[..take]);
            off += take;
            if off >= out.len() {
                break;
            }
            cluster = self.next_cluster(disk, cluster)?;
        }
        Some(out)
    }
}
