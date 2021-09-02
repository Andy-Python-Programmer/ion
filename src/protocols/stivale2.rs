use crate::logger;
use crate::BootPageTables;

use raw_cpuid::CpuId;
use stivale_boot::v2::StivaleHeader;

use x86_64::structures::paging::*;
use xmas_elf::program::ProgramHeader;

fn handle_load_segment(header: ProgramHeader) {}

pub fn boot(
    offset_table: &mut BootPageTables,
    allocator: &mut impl FrameAllocator<Size4KiB>,
    kernel: &'static [u8],
) {
    logger::clear();
    logger::flush();

    let elf = xmas_elf::ElfFile::new(kernel).expect("stivale2: invalid ELF file");

    let stivale2_hdr;
    let is_32_bit = false;

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
                    xmas_elf::program::Type::Load => handle_load_segment(p_header),
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
