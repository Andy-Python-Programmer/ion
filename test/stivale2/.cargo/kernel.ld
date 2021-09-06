OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
    null    PT_NULL    FLAGS(0) ;                   /* Null segment */
    text    PT_LOAD    FLAGS((1 << 0) | (1 << 2)) ; /* Execute + Read */
    rodata  PT_LOAD    FLAGS((1 << 2)) ;            /* Read only */
    data    PT_LOAD    FLAGS((1 << 1) | (1 << 2)) ; /* Write + Read */
}

/* We want to be placed in the higher half, 2MiB above 0x00 in physical memory */
KERNEL_OFFSET = 0xFFFFFFFF80200000;

SECTIONS {
    . = KERNEL_OFFSET;

    .text : {
        *(.text*)
    } :text

    /* Move to the next memory page for .rodata */
    . += 0x1000;

    /* We place the .stivale2hdr section containing the header in its own section, */
    /* and we use the KEEP directive on it to make sure it doesn't get discarded. */
    .stivale2hdr : {
        KEEP(*(.stivale2hdr))
    } :rodata

    .rodata : {
        *(.rodata*)
    } :rodata

    /* Move to the next memory page for .data */
    . += 0x1000;

    .data : {
        *(.data*)
    } :data

    .bss : {
        *(COMMON)
        *(.bss*)
    } :data
}
