#![feature(abi_efiapi, custom_test_frameworks, asm)]
#![test_runner(crate::test_runner)]
#![no_std]
#![no_main]

use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;

use core::fmt::Write;
use core::panic::PanicInfo;

mod logger;

/// This function is responsible for initializing the logger for Ion and
/// returns the physical address of the framebuffer.
fn init_logger(system_table: &SystemTable<Boot>) {
    let gop = system_table
        .boot_services()
        .locate_protocol::<GraphicsOutput>()
        .expect_success("failed to locate GOP");

    let gop = unsafe { &mut *gop.get() };
    let mode_info = gop.current_mode_info();
    let (horizontal_resolution, vertical_resolution) = mode_info.resolution();

    let mut framebuffer = gop.frame_buffer();

    let slice =
        unsafe { core::slice::from_raw_parts_mut(framebuffer.as_mut_ptr(), framebuffer.size()) };

    let info = logger::FrameBufferInfo {
        horizontal_resolution,
        vertical_resolution,
        pixel_format: match mode_info.pixel_format() {
            uefi::proto::console::gop::PixelFormat::Rgb => logger::PixelFormat::RGB,
            uefi::proto::console::gop::PixelFormat::Bgr => logger::PixelFormat::BGR,
            _ => unimplemented!(),
        },
        bits_per_pixel: 4,
        stride: mode_info.stride(),
    };

    logger::init(slice, info)
}

#[entry]
fn efi_main(image_handle: Handle, system_table: SystemTable<Boot>) -> Status {
    system_table
        .stdout()
        .clear()
        .expect_success("Failed to clear system stdout");

    init_logger(&system_table);

    log::debug!("Hola!");

    loop {}
}

#[panic_handler]
extern "C" fn rust_begin_unwind(info: &PanicInfo) -> ! {
    use uart_16550::SerialPort;

    const SERIAL_IO_PORT: u16 = 0x3F8;

    let mut serial_port = unsafe { SerialPort::new(SERIAL_IO_PORT) };
    serial_port.init();

    writeln!(serial_port, "{}", info).unwrap();

    loop {}
}

#[cfg(test)]
fn test_runner(tests: &[&dyn Fn()]) {
    for test in tests {
        test();
    }
}
