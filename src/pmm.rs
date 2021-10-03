use uefi::table::boot::{MemoryDescriptor, MemoryType};

use x86_64::structures::paging::*;
use x86_64::{PhysAddr, VirtAddr};
use xmas_elf::program::ProgramHeader;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
#[repr(C)]
pub enum MemoryRegionType {
    /// Unused conventional memory, can be used by the kernel.
    Usable,
    UnknownUefi(u32),
}

/// Represent a physical memory region.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub struct MemoryRegion {
    /// The physical start address of the region.
    pub start: u64,
    /// The physical end address (exclusive) of the region.
    pub end: u64,
    /// The memory type of the memory region.
    pub kind: MemoryRegionType,
}

pub trait BootMemoryRegion: Copy + core::fmt::Debug {
    /// Returns the physical start address of the region.
    fn start(&self) -> PhysAddr;

    /// Returns the size of the region in bytes.
    fn len(&self) -> u64;

    /// Returns the type of the region
    fn region_type(&self) -> MemoryRegionType;
}

impl<'a> BootMemoryRegion for MemoryDescriptor {
    fn start(&self) -> PhysAddr {
        PhysAddr::new(self.phys_start)
    }

    fn len(&self) -> u64 {
        self.page_count * Size4KiB::SIZE
    }

    fn region_type(&self) -> MemoryRegionType {
        match self.ty {
            MemoryType::CONVENTIONAL => MemoryRegionType::Usable,
            other => MemoryRegionType::UnknownUefi(other.0),
        }
    }
}

pub struct BootFrameAllocator<I, D> {
    #[allow(unused)]
    original: I,
    memory_map: I,
    current_descriptor: Option<D>,
    next_frame: PhysFrame,
}

impl<I, D> BootFrameAllocator<I, D>
where
    I: ExactSizeIterator<Item = D> + Clone,
    I::Item: BootMemoryRegion,
{
    pub fn new(memory_map: I) -> Self {
        let start_frame = PhysFrame::containing_address(PhysAddr::new(0x1000));

        Self {
            original: memory_map.clone(),
            memory_map,
            current_descriptor: None,
            next_frame: start_frame,
        }
    }

    pub fn allocate_frame_from_descriptor(&mut self, descriptor: D) -> Option<PhysFrame> {
        let start_addr = descriptor.start();
        let start_frame = PhysFrame::containing_address(start_addr);
        let end_addr = start_addr + descriptor.len();
        let end_frame = PhysFrame::containing_address(end_addr - 1u64);

        // Set next_frame to start_frame if its smaller then next_frame.
        if self.next_frame < start_frame {
            self.next_frame = start_frame;
        }

        if self.next_frame < end_frame {
            let frame = self.next_frame;
            self.next_frame += 1;

            Some(frame)
        } else {
            None
        }
    }

    /// Returns the number of memory regions in the underlying memory map.
    ///
    /// The function always returns the same value, i.e. the length doesn't
    /// change after calls to `allocate_frame`.
    pub fn len(&self) -> usize {
        self.original.len()
    }

    /// Returns the largest detected physical memory address.
    ///
    /// Useful for creating a mapping for all physical memory.
    pub fn max_phys_addr(&self) -> PhysAddr {
        self.original
            .clone()
            .map(|r| r.start() + r.len())
            .max()
            .unwrap()
    }
}

unsafe impl<I, D> FrameAllocator<Size4KiB> for BootFrameAllocator<I, D>
where
    I: ExactSizeIterator<Item = D> + Clone,
    I::Item: BootMemoryRegion,
{
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        if let Some(current_descriptor) = self.current_descriptor {
            match self.allocate_frame_from_descriptor(current_descriptor) {
                Some(frame) => return Some(frame),
                None => {
                    self.current_descriptor = None;
                }
            }
        }

        // Find next suitable descriptor
        while let Some(descriptor) = self.memory_map.next() {
            if descriptor.region_type() != MemoryRegionType::Usable {
                continue;
            }

            if let Some(frame) = self.allocate_frame_from_descriptor(descriptor) {
                self.current_descriptor = Some(descriptor);
                return Some(frame);
            }
        }

        None
    }
}

/// Keeps track of used entries in a level 4 page table.
///
/// Useful for determining a free virtual memory block, e.g. for mapping additional data.
pub struct UsedLevel4Entries {
    entry_state: [bool; 512], // whether an entry is in use by the kernel
}

impl UsedLevel4Entries {
    /// Initializes a new instance from the given ELF program segments.
    ///
    /// Marks the virtual address range of all segments as used.
    pub fn new<'a>(segments: impl Iterator<Item = ProgramHeader<'a>>) -> Self {
        let mut used = UsedLevel4Entries {
            entry_state: [false; 512],
        };

        used.entry_state[0] = true; // TODO: Can we do this dynamically?

        for segment in segments {
            let start_page: Page = Page::containing_address(VirtAddr::new(segment.virtual_addr()));
            let end_page: Page = Page::containing_address(VirtAddr::new(
                segment.virtual_addr() + segment.mem_size(),
            ));

            for p4_index in u64::from(start_page.p4_index())..=u64::from(end_page.p4_index()) {
                used.entry_state[p4_index as usize] = true;
            }
        }

        used
    }

    /// Returns a unused level 4 entry and marks it as used.
    ///
    /// Since this method marks each returned index as used, it can be used multiple times
    /// to determine multiple unused virtual memory regions.
    pub fn get_free_entry(&mut self) -> PageTableIndex {
        let (idx, entry) = self
            .entry_state
            .iter_mut()
            .enumerate()
            .find(|(_, &mut entry)| entry == false)
            .expect("no usable level 4 entries found");

        *entry = true;
        PageTableIndex::new(idx as u16)
    }

    /// Returns the virtual start address of an unused level 4 entry and marks it as used.
    ///
    /// This is a convenience method around [`get_free_entry`], so all of its docs applies here
    /// too.
    pub fn get_free_address(&mut self) -> VirtAddr {
        Page::from_page_table_indices_1gib(self.get_free_entry(), PageTableIndex::new(0))
            .start_address()
    }
}
