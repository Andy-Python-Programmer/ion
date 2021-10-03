all: stivale2.elf

stivale2.elf:
	@ cargo build
	@ cp target/x86_64-unknown/debug/stivale2 ../../build/stivale2.elf

clean:
	@ rm -f ../../build/stivale2.elf
	@ cargo clean
