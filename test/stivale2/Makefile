all: stivale2.elf

stivale2.elf:
	@ cargo build --release
	@ cp target/x86_64-unknown/release/stivale2 ../../build/stivale2.elf

clean:
	@ rm -f ../../build/stivale2.elf
	@ cargo clean
