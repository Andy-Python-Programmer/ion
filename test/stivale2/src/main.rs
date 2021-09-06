#![no_std]
#![no_main]

use core::panic::PanicInfo;

use stivale_boot::v2::*;

#[repr(C, align(4096))]
struct P2Align12<T>(T);

const STACK_SIZE: usize = 4096 * 16;

/// We need to tell the stivale bootloader where we want our stack to be.
/// We are going to allocate our stack as an uninitialised array in .bss.
static STACK: P2Align12<[u8; STACK_SIZE]> = P2Align12([0; STACK_SIZE]);

/// The stivale2 specification says we need to define a "header structure".
/// This structure needs to reside in the .stivale2hdr ELF section in order
/// for the bootloader to find it. We use the #[linker_section] and #[used] macros to
/// tell the compiler to put the following structure in said section.
#[link_section = ".stivale2hdr"]
#[no_mangle]
#[used]
static STIVALE_HDR: StivaleHeader = StivaleHeader::new()
    .stack(&STACK.0[STACK_SIZE - 4096] as *const u8)
    .tags(0x00 as *const ());

#[no_mangle]
extern "C" fn _start() -> ! {
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
