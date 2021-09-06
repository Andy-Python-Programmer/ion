use crate::logger;
use crate::BootPageTables;

use raw_cpuid::CpuId;
use stivale_boot::v2::StivaleHeader;

use x86_64::align_up;
use x86_64::registers::control::Cr0;
use x86_64::registers::control::Cr0Flags;
use x86_64::registers::model_specific::Efer;
use x86_64::registers::model_specific::EferFlags;
use x86_64::structures::paging::*;
use x86_64::PhysAddr;
use x86_64::VirtAddr;

use x86_64::structures::paging::mapper::MapToError;
use xmas_elf::program::ProgramHeader;

fn handle_bss_segment(
    segment: &ProgramHeader,
    segment_flags: PageTableFlags,
    kernel_offset: PhysAddr,
    page_table: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let virt_start_addr = VirtAddr::new(segment.virtual_addr());
    let phys_start_addr = kernel_offset + segment.offset();
    let mem_size = segment.mem_size();
    let file_size = segment.file_size();

    // Calculate virual memory region that must be zeroed
    let zero_start = virt_start_addr + file_size;
    let zero_end = virt_start_addr + mem_size;

    // A type alias that helps in efficiently clearing a page
    type PageArray = [u64; Size4KiB::SIZE as usize / 8];
    const ZERO_ARRAY: PageArray = [0; Size4KiB::SIZE as usize / 8];

    // In some cases, `zero_start` might not be page-aligned. This requires some
    // special treatment because we can't safely zero a frame of the original file.
    let data_bytes_before_zero = zero_start.as_u64() & 0xfff;
    if data_bytes_before_zero != 0 {
        /*
         * The last non-bss frame of the segment consists partly of data and partly of bss
         * memory, which must be zeroed. Unfortunately, the file representation might have
         * reused the part of the frame that should be zeroed to store the next segment. This
         * means that we can't simply overwrite that part with zeroes, as we might overwrite
         * other data this way.
         *
         * Example:
         *
         *   XXXXXXXXXXXXXXX000000YYYYYYY000ZZZZZZZZZZZ     virtual memory (XYZ are data)
         *   |·············|     /·····/   /·········/
         *   |·············| ___/·····/   /·········/
         *   |·············|/·····/‾‾‾   /·········/
         *   |·············||·····|/·̅·̅·̅·̅·̅·····/‾‾‾‾
         *   XXXXXXXXXXXXXXXYYYYYYYZZZZZZZZZZZ              file memory (zeros are not saved)
         *   '       '       '       '        '
         *   The areas filled with dots (`·`) indicate a mapping between virtual and file
         *   memory. We see that the data regions `X`, `Y`, `Z` have a valid mapping, while
         *   the regions that are initialized with 0 have not.
         *
         *   The ticks (`'`) below the file memory line indicate the start of a new frame. We
         *   see that the last frames of the `X` and `Y` regions in the file are followed
         *   by the bytes of the next region. So we can't zero these parts of the frame
         *   because they are needed by other memory regions.
         *
         * To solve this problem, we need to allocate a new frame for the last segment page
         * and copy all data content of the original frame over. Afterwards, we can zero
         * the remaining part of the frame since the frame is no longer shared with other
         * segments now.
         */

        // Calculate the frame where the last segment page is mapped
        let orig_frame: PhysFrame =
            PhysFrame::containing_address(phys_start_addr + file_size - 1u64);

        // Allocate a new frame to replace `orig_frame`
        let new_frame = frame_allocator.allocate_frame().unwrap();

        // Zero new frame, utilizing that it's identity-mapped
        {
            let new_frame_ptr = new_frame.start_address().as_u64() as *mut PageArray;
            unsafe { new_frame_ptr.write(ZERO_ARRAY) };
        }

        // Copy the data bytes from orig_frame to new_frame
        {
            log::info!("Copy contents");
            let orig_bytes_ptr = orig_frame.start_address().as_u64() as *mut u8;
            let new_bytes_ptr = new_frame.start_address().as_u64() as *mut u8;

            for offset in 0..(data_bytes_before_zero as isize) {
                unsafe {
                    let orig_byte = orig_bytes_ptr.offset(offset).read();
                    new_bytes_ptr.offset(offset).write(orig_byte);
                }
            }
        }

        // Remap last page from orig_frame to `new_frame`
        log::info!("Remap last page");

        let last_page = Page::containing_address(virt_start_addr + file_size - 1u64);

        // SAFETY: We operate on an inactive page table, so we don't need to flush our changes
        page_table.unmap(last_page.clone()).unwrap().1.ignore();

        let flusher =
            unsafe { page_table.map_to(last_page, new_frame, segment_flags, frame_allocator) }?;

        // SAFETY: We operate on an inactive page table, so we don't need to flush our changes
        flusher.ignore();
    }

    // Map additional frames for `.bss` memory that is not present in source file
    let start_page: Page =
        Page::containing_address(VirtAddr::new(align_up(zero_start.as_u64(), Size4KiB::SIZE)));
    let end_page = Page::containing_address(zero_end);

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator.allocate_frame().unwrap();

        // Zero frame, utilizing identity-mapping
        let frame_ptr = frame.start_address().as_u64() as *mut PageArray;
        unsafe { frame_ptr.write(ZERO_ARRAY) };

        let flusher = unsafe { page_table.map_to(page, frame, segment_flags, frame_allocator)? };

        // SAFETY: We operate on an inactive page table, so we don't need to flush our changes
        flusher.ignore();
    }

    Ok(())
}

fn handle_load_segment(
    segment: ProgramHeader,
    kernel_offset: PhysAddr,
    page_table: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let phys_start_addr = kernel_offset + segment.offset();
    let start_frame: PhysFrame = PhysFrame::containing_address(phys_start_addr);
    let end_frame: PhysFrame =
        PhysFrame::containing_address(phys_start_addr + segment.file_size() - 1u64);

    let virt_start_addr = VirtAddr::new(segment.virtual_addr());
    let start_page: Page = Page::containing_address(virt_start_addr);

    let mut segment_flags = PageTableFlags::PRESENT;

    if !segment.flags().is_execute() {
        segment_flags |= PageTableFlags::NO_EXECUTE;
    }

    if segment.flags().is_write() {
        segment_flags |= PageTableFlags::WRITABLE;
    }

    // Map all frames of the segment at the desired virtual address.
    for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
        let offset = frame - start_frame;
        let page = start_page + offset;

        let flusher = unsafe { page_table.map_to(page, frame, segment_flags, frame_allocator) }?;
        // We operate on an inactive page table, so there's no need to flush anything :^)
        flusher.ignore();
    }

    if segment.mem_size() > segment.file_size() {
        handle_bss_segment(
            &segment,
            segment_flags,
            kernel_offset,
            page_table,
            frame_allocator,
        )?;
    }

    Ok(())
}

pub fn boot(
    page_tables: &mut BootPageTables,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    kernel: &'static [u8],
) {
    logger::clear();
    logger::flush();

    let kernel_offset = unsafe { PhysAddr::new_unsafe(&kernel[0] as *const u8 as u64) };
    assert!(
        kernel_offset.is_aligned(Size4KiB::SIZE),
        "stivale2: loaded kernel ELF file is not sufficiently aligned"
    );

    let elf = xmas_elf::ElfFile::new(kernel).expect("stivale2: invalid ELF file");

    let stivale2_hdr;
    let is_32_bit = false;

    enable_nxe_bit();
    enable_write_protect_bit();

    match elf.header.pt2.machine().as_machine() {
        xmas_elf::header::Machine::X86_64 => {
            // 1. Check if the CPU actually supports long mode.
            let long_mode_supported = CpuId::new()
                .get_extended_processor_and_feature_identifiers()
                .map_or(false, |info| info.has_64bit_mode());

            if !long_mode_supported {
                panic!("stivale2: CPU does not support 64-bit mode.")
            }

            xmas_elf::header::sanity_check(&elf).expect("stivale2: failed ELF sanity check");

            // 2. Get the stivale2 header section.
            let header = elf
                .find_section_by_name(".stivale2hdr")
                .expect("stivale2: section .stivale2hdr not found");

            if header.size() < core::mem::size_of::<StivaleHeader>() as u64 {
                panic!("stivale2: section .stivale2hdr is smaller than size of the struct.");
            } else if header.size() > core::mem::size_of::<StivaleHeader>() as u64 {
                panic!("stivale2: section .stivale2hdr is larger than size of the struct.");
            }

            // SAFETY: The size of the section is checked above and the address provided is valid and
            // mapped.
            stivale2_hdr = unsafe { &*(header.raw_data(&elf).as_ptr() as *const StivaleHeader) };

            log::info!("stivale2: 64-bit kernel detected");

            // 3. Load the kernel.
            for p_header in elf.program_iter() {
                xmas_elf::program::sanity_check(p_header, &elf)
                    .expect("stivale2: failed ELF program header sanity check");

                match p_header
                    .get_type()
                    .expect("stivale2: failed to get ELF program heade type")
                {
                    xmas_elf::program::Type::Load => handle_load_segment(
                        p_header,
                        kernel_offset,
                        &mut page_tables.kernel,
                        frame_allocator,
                    )
                    .unwrap(),
                    _ => {}
                }
            }
        }

        machine => panic!("stivale2: unsupported architecture {:?}", machine),
    };

    if (stivale2_hdr.get_flags() & (1 << 1)) == 1 && is_32_bit {
        panic!("stivale2: higher half header flag not supported in 32-bit mode");
    }

    // The stivale2 specs says the stack has to be 16-byte aligned.
    if (stivale2_hdr.get_stack() as u64 & (16 - 1)) != 0 {
        panic!("stivale2: requested stack is not 16-byte aligned");
    }

    // It also says the stack cannot be NULL for 32-bit kernels
    if is_32_bit && stivale2_hdr.get_stack() as u64 == 0 {
        panic!("stivale2: the stack cannot be 0 for 32-bit kernels");
    }

    // Now we have to prepare the stivale struct that we will pass as an argument
    // in RDI to the kernel's entry point function.

    logger::flush();
}

fn enable_nxe_bit() {
    unsafe { Efer::update(|efer| *efer |= EferFlags::NO_EXECUTE_ENABLE) }
}

fn enable_write_protect_bit() {
    unsafe { Cr0::update(|cr0| *cr0 |= Cr0Flags::WRITE_PROTECT) };
}
