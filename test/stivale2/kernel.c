#include <stivale2.h>
#include <stdint.h>
#include <stddef.h>

// We need to tell the stivale bootloader where we want our stack to be.
// We are going to allocate our stack as an uninitialised array in .bss.
typedef uint8_t stack[4096];

static stack stacks[10] = {0};

// We are now going to define a framebuffer header tag/
struct stivale2_header_tag_framebuffer framebuffer_request = {
    .tag = {
        .identifier = STIVALE2_HEADER_TAG_FRAMEBUFFER_ID,
        // If next is 0, it marks the end of the linked list of header tags.
        .next       = 0,
    },

    // We set all the framebuffer specifics to 0 as we want the ion
    // to pick the best it can.
    .framebuffer_width  = 0,
    .framebuffer_height = 0,
    .framebuffer_bpp    = 0,
};

__attribute__((section(".stivale2hdr"), used))
struct stivale2_header header2 = {
    .stack       = (uintptr_t)stacks[0] + sizeof(stack),
    // Bit 1: If set, causes the bootloader to return to us pointers in the
    // higher half, which we likely want.
    // Bit 2: If set, tells the bootloader to enable protected memory ranges,
    // that is, to respect the ELF PHDR mandated permissions for the executable's
    // segments.
    .flags       = (1 << 1) | (1 << 2),
    .tags        = (uint64_t)&framebuffer_request
};

// The following will be our kernel's entry point.
void stivale2_main(struct stivale_struct *info) {
    while (1);
}
