ENTRY(stivale_main)
OUTPUT_FORMAT(elf64-x86-64)

/* We want to be placed in the higher half, 2MiB above 0x00 in physical memory */
KERNEL_OFFSET = 0xFFFFFFFF80200000;

SECTIONS {
    . = KERNEL_OFFSET;

    . += SIZEOF_HEADERS;
    . = ALIGN(4096);

    __kernel_start = .;

    .stivale2hdr : ALIGN(4K) {
        KEEP(*(.stivale2hdr))
    }

    /* Then place all of the other traditional executable sections afterwards... */
    .text : {
        __text_start = .;
        *(.text*)
        . = ALIGN(4096);
        __text_end = .;
    }

    .rodata : {
        __rodata_start = .;
        *(.rodata*)
        . = ALIGN(4096);
        __rodata_end = .;
    }

    .data : {
        __data_start = .;
        *(.data*)
        *(.got*)
        . = ALIGN(4096);
        __data_end = .;
    }

    .bss : {
        __bss_start = .;
        *(.bss*)
        . = ALIGN(4096);
        __bss_end = .;
    }
}
