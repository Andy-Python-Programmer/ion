#![feature(
    abi_efiapi,
    custom_test_frameworks,
    asm,
    panic_info_message,
    alloc_error_handler,
    option_result_unwrap_unchecked
)]
#![test_runner(crate::test_runner)]
#![no_std]
#![no_main]

extern crate alloc;

use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::table::boot::MemoryType;

use core::panic::PanicInfo;

mod config;
mod logger;
mod menu;
mod protocols;
mod prelude {
    pub use crate::{print, println};
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("oom {:?}", layout)
}

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

    let backbuffer = unsafe {
        let ptr = system_table
            .boot_services()
            .allocate_pool(MemoryType::LOADER_DATA, framebuffer.size())
            .expect_success("could not allocate memory");

        // SAFETY: The provided pointer by allocate_pool is guaranteed to be
        // valid.
        core::slice::from_raw_parts_mut(ptr, framebuffer.size())
    };

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

    logger::init(slice, backbuffer, info)
}

#[entry]
fn efi_main(image_handle: Handle, system_table: SystemTable<Boot>) -> Status {
    system_table
        .stdout()
        .clear()
        .expect_success("failed to clear system stdout");

    init_logger(&system_table);

    let boot_services = system_table.boot_services();

    unsafe {
        // SAFETY: We invoke exit_boot_services in alloc when we are done with the
        // boot services.
        uefi::alloc::init(boot_services);
    }

    // Query the handle for the loaded image protocol.
    let loaded_image = system_table
        .boot_services()
        .handle_protocol::<LoadedImage>(image_handle)
        .expect_success("failed to retrieve loaded image protocokl");
    let loaded_image = unsafe { &*loaded_image.get() }; // Get the inner cell value

    // Query the handle for the simple file system protocol.
    let filesystem = system_table
        .boot_services()
        .handle_protocol::<SimpleFileSystem>(loaded_image.device())
        .expect_success("failed to retrieve simple file system to read disk");
    let filesystem = unsafe { &mut *filesystem.get() }; // Get the inner cell value

    // Open the root directory of the simple file system volume.
    let mut root = filesystem
        .open_volume()
        .expect_success("failed to open volume");

    let ion_config = config::load(&system_table, &mut root); // Load the config and store it in a local variable.
    let selected_entry = menu::init(&system_table, ion_config);

    match selected_entry.protocol() {
        config::BootProtocol::Stivale2 => {
            protocols::stivale2::boot(&system_table, &mut root, selected_entry)
        }

        config::BootProtocol::Stivale => todo!(),
        config::BootProtocol::Multiboot => todo!(),
        config::BootProtocol::Multiboot2 => todo!(),
        config::BootProtocol::Linux => todo!(),
    }

    loop {}
}

#[panic_handler]
extern "C" fn rust_begin_unwind(info: &PanicInfo) -> ! {
    unsafe {
        logger::LOGGER.get().map(|l| l.force_unlock());
    }

    let deafult_panic = &format_args!("");
    let panic_message = info.message().unwrap_or(deafult_panic);

    log::error!("cpu '0' panicked at '{}'", panic_message);

    if let Some(panic_location) = info.location() {
        log::error!("{}", panic_location);
    }

    logger::flush();

    unsafe {
        asm!("cli");

        loop {
            asm!("hlt");
        }
    }
}

#[cfg(test)]
fn test_runner(tests: &[&dyn Fn()]) {
    for test in tests {
        test();
    }
}
