#![feature(
    abi_efiapi,
    custom_test_frameworks,
    asm,
    panic_info_message,
    alloc_error_handler,
    maybe_uninit_slice
)]
#![test_runner(crate::test_runner)]
#![no_std]
#![no_main]

extern crate alloc;

use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode, RegularFile};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::table::boot::{AllocateType, MemoryDescriptor, MemoryType};
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::*;
use x86_64::VirtAddr;

use core::mem;
use core::panic::PanicInfo;

mod config;
mod logger;
mod menu;
mod pmm;
mod protocols;
mod prelude {
    pub use crate::{print, println};
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("oom {:?}", layout)
}

pub struct BootPageTables {
    /// Provides access to the page tables of the bootloader address space.
    pub bootloader: OffsetPageTable<'static>,
    /// Provides access to the page tables of the kernel address space (not active).
    pub kernel: OffsetPageTable<'static>,
    /// The physical frame where the level 4 page table of the kernel address space is stored.
    pub kernel_level_4_frame: PhysFrame,
}

/// Helper function to create and load the bootloader's page table and
/// create a new page table for the kernel itself.
fn setup_boot_paging(frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> BootPageTables {
    // NOTE: UEFI identity-maps all memory, so the offset between physical
    // and virtual addresses is 0.
    let off = VirtAddr::zero();

    let boot_page_table = {
        let old_table = {
            let (frame, _) = Cr3::read();

            let ptr: *const PageTable = (off + frame.start_address().as_u64()).as_ptr();

            unsafe { &*ptr }
        };

        let new_frame = frame_allocator
            .allocate_frame()
            .expect("mm: failed to allocate frame for new level 4 boot table");

        let new_table: &mut PageTable = {
            let ptr: *mut PageTable = (off + new_frame.start_address().as_u64()).as_mut_ptr();

            unsafe {
                // Create a new empty, fresh page table.
                ptr.write(PageTable::new());
                &mut *ptr
            }
        };

        // Copy the first entry (we don't need to access more than 512 GiB; also, some UEFI
        // implementations seem to create an level 4 table entry 0 in all slots)
        new_table[0] = old_table[0].clone();

        // The first level 4 table entry is now identical, so we can just load the new one.
        unsafe {
            Cr3::write(new_frame, Cr3Flags::empty());
            OffsetPageTable::new(&mut *new_table, off)
        }
    };

    // Now we will create a page table for the kernel itself.
    let (kernel_page_table, kernel_level_4_frame) = {
        // get an unused frame for new level 4 page table
        let frame: PhysFrame = frame_allocator
            .allocate_frame()
            .expect("mm: no unused frames");

        log::info!("new page table at: {:#?}", &frame);

        // 1. Get the corresponding virtual address.
        let addr = off + frame.start_address().as_u64();

        // 2. Initialize a new page table.
        let ptr = addr.as_mut_ptr();
        unsafe { *ptr = PageTable::new() };
        let level_4_table = unsafe { &mut *ptr };

        (unsafe { OffsetPageTable::new(level_4_table, off) }, frame)
    };

    BootPageTables {
        bootloader: boot_page_table,
        kernel: kernel_page_table,
        kernel_level_4_frame,
    }
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

fn prepare_kernel(
    system_table: &SystemTable<Boot>,
    root: &mut Directory,
    entry: &config::ConfigurationEntry,
) -> &'static [u8] {
    let parsed_uri = config::parse_uri(entry.path()).expect("stivale2: failed to parse the URI");
    let uri = config::handle_uri_redirect(&parsed_uri, root);

    assert_ne!(entry.path().len(), 0, "stivale2: KERNEL_PATH not specified");

    let kernel_path = entry.path();
    let file_completion = uri
        .open(parsed_uri.path(), FileMode::Read, FileAttribute::empty())
        .expect_success("stivale2: failed to open kernel file. Is its path correct?");

    log::debug!("stivale2: loading kernel {}...\n", kernel_path);

    let mut cfg_file_handle = unsafe { RegularFile::new(file_completion) };

    let mut info_buf = [0; 0x100];
    let cfg_info = cfg_file_handle
        .get_info::<FileInfo>(&mut info_buf)
        .unwrap_success();

    let pages = cfg_info.file_size() as usize / 0x1000 + 1;
    let mem_start = system_table
        .boot_services()
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
        .unwrap_success();

    let buf = unsafe { core::slice::from_raw_parts_mut(mem_start as *mut u8, pages * 0x1000) };
    let len = cfg_file_handle.read(buf).unwrap_success();

    buf[..len].as_ref()
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

    // We have to load the kernel before we exit the boot services since we rely on the
    // simple file system boot services protocol to read the kernel from the disk into
    // memory.
    let kernel = prepare_kernel(&system_table, &mut root, &selected_entry);

    let mmap_storage = {
        let max_mmap_size =
            system_table.boot_services().memory_map_size() + 8 * mem::size_of::<MemoryDescriptor>();

        let ptr = system_table
            .boot_services()
            .allocate_pool(MemoryType::LOADER_DATA, max_mmap_size)
            .expect_success("dispatch: failed to allocate pool for memory map");

        unsafe { core::slice::from_raw_parts_mut(ptr, max_mmap_size) }
    };

    uefi::alloc::exit_boot_services();

    let (_, mmap) = system_table
        .exit_boot_services(image_handle, mmap_storage)
        .expect_success("ion: failed to exit the boot services");

    logger::clear();
    logger::flush();

    let mut allocator = pmm::BootFrameAllocator::new(mmap.copied());
    let mut offset_tables = setup_boot_paging(&mut allocator);

    match selected_entry.protocol() {
        config::BootProtocol::Stivale2 => {
            protocols::stivale2::boot(&mut offset_tables, &mut allocator, kernel)
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
