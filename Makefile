KERNEL       := target/x86_64-unknown-none/release/kernel
LIMINE_BRANCH ?= v9.x-binary
OVMF         ?= /usr/share/ovmf/OVMF.fd
DISK_SIZE_MB ?= 64

.PHONY: all run clean

all: image.iso disk.img

$(KERNEL): $(shell find src assets -type f) Cargo.toml build.rs linker.ld
	cargo build --release

limine:
	git clone https://github.com/limine-bootloader/limine.git --branch=$(LIMINE_BRANCH) --depth=1
	$(MAKE) -C limine

image.iso: $(KERNEL) limine.conf | limine
	rm -rf iso_root
	mkdir -p iso_root/boot/limine iso_root/EFI/BOOT
	cp $(KERNEL) iso_root/boot/kernel
	cp limine.conf limine/limine-bios.sys limine/limine-bios-cd.bin limine/limine-uefi-cd.bin iso_root/boot/limine/
	cp limine/BOOTX64.EFI iso_root/EFI/BOOT/
	xorriso -as mkisofs -R -r -J -b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table -hfsplus \
		-apm-block-size 2048 --efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		iso_root -o image.iso
	./limine/limine bios-install image.iso

# Диск A: без таблицы разделов («суперфлоппи») — FAT32 начинается с LBA 0.
# Пересобирается только если удалить файл: правки внутри образа не пропадут
# при обычной сборке ядра.
disk.img:
	dd if=/dev/zero of=disk.img bs=1M count=$(DISK_SIZE_MB) status=none
	mkfs.vfat -F32 -n HORIZON disk.img
	mcopy -i disk.img assets/sounds/STARTUP.WAV ::STARTUP.WAV
	mcopy -i disk.img assets/sounds/SHUTDOWN.WAV ::SHUTDOWN.WAV

# UEFI-загрузка в QEMU. -rtc base=localtime: часы показывают местное время.
# Мышь захватывается кликом по окну, освободить: Ctrl+Alt+G.
# Диск A: подключён как AHCI (ich9-ahci), тот же контроллер ищет pci.rs.
# Звук: AC97 через ALSA. Если звука нет (например, только PipeWire без
# ALSA-моста), замени alsa на pa: `make run AUDIODEV=pa`.
AUDIODEV ?= alsa
run: image.iso disk.img
	qemu-system-x86_64 -M q35 -m 256M -bios $(OVMF) -cdrom image.iso -rtc base=localtime \
		-drive id=disk,file=disk.img,if=none,format=raw \
		-device ahci,id=ahci0 -device ide-hd,drive=disk,bus=ahci0.0 \
		-audiodev $(AUDIODEV),id=snd0 -device AC97,audiodev=snd0

clean:
	cargo clean
	rm -rf iso_root image.iso disk.img
