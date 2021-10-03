.PHONY: uefi-stivale2-test
.PHONY: clean
.PHONY: ovmf-x64

# Downloads the latest prebuilt UEFI OVMF binaries for x86_64 into the ovmf
# directory.
ovmf-x64:
	@ mkdir -p ovmf
	@ test -f ovmf/OVMF-pure-efi.fd \
		|| curl --location https://github.com/rust-osdev/ovmf-prebuilt/releases/latest/download/OVMF-pure-efi.fd \
		--output ovmf/OVMF-pure-efi.fd

	@ echo "\033[32;1mOK:\033[0m Downloaded latest OVMF prebuilt binaries..."

# Builds and runs the stivale 2 test kernel in Qemu using UEFI.
uefi-stivale2-test:
	@ test -d build && rm -rf build
	@ mkdir build

	@ $(MAKE) ovmf-x64 --no-print-directory
	@ $(MAKE) -C test/stivale2 --no-print-directory
	@ echo "\033[32;1mOK:\033[0m Built UEFI stivale 2 test kernel..."

	@ cargo build --release
	@ dd if=/dev/zero bs=1M count=0 seek=64 of=build/ion.hdd status=none

	@ parted -s build/ion.hdd mklabel gpt
	@ parted -s build/ion.hdd mkpart primary 2048s 100%

	@ mkdir build/mnt

	@ sudo losetup -Pf --show build/ion.hdd > loopback_dev
	@ sudo mkfs.fat -F 32 `cat loopback_dev`p1 > /dev/null
	@ sudo mount `cat loopback_dev`p1 build/mnt

	@ sudo mkdir -p build/mnt/EFI/BOOT
	@ sudo mkdir -p build/mnt/boot

	@ sudo cp ./target/x86_64-unknown-uefi/release/ion.efi build/mnt/EFI/BOOT/BOOTX64.EFI
	@ sudo cp ./ion.cfg build/mnt/ion.cfg
	@ sudo cp ./build/stivale2.elf build/mnt/boot/

	@ sync

	@ sudo umount build/mnt
	@ sudo losetup -d `cat loopback_dev`

	@ rm -rf build/mnt/ loopback_dev

	@ printf '\033[32;1mOK:\033[0m Running UEFI stivale2 test kernel in Qemu...'
	@ qemu-system-x86_64 -machine type=q35 -serial stdio -drive format=raw,file=build/ion.hdd \
		-bios ../aero/bundled/ovmf/OVMF-pure-efi.fd \
		-d int \
		-D qemulog.uefi.log \
		--no-reboot

# Clean up build directory.
clean:
	@ cargo clean
	@ echo "\033[32;1mOK:\033[0m Cleaned Ion build..."

	@ $(MAKE) -C test/stivale2 clean --no-print-directory
	@ echo "\033[32;1mOK:\033[0m Cleaned stivale2 test kernel build..."

	@ rm -rf build/ion.hdd
