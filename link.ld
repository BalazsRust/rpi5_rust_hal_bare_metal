/*
 * Linker script for pi5-hal: a bare-metal AArch64 image for the Raspberry Pi 5.
 *
 * What the firmware does for us
 * -----------------------------
 * The Pi 5 bootloader (in the on-board EEPROM) reads `kernel8.img` from the
 * boot partition, copies it to physical address 0x80000 (the default
 * `kernel_address` for 64-bit images) and jumps to its FIRST byte on CPU
 * core 0, in EL2, with the MMU and caches off and x0 = address of the
 * device tree blob. There is no ELF loader: it is a flat binary, so
 * whatever we put at offset 0 of the file is what runs first. That is why
 * `_start` must be the first thing in `.text` (see KEEP below) and why the
 * image must be built to run at 0x80000.
 *
 * `llvm-objcopy -O binary` turns the ELF produced with this script into the
 * flat file: it writes every *loadable* section, from the lowest to the
 * highest address, into the file. NOLOAD sections (.bss, .stack) take no
 * space in the file; the startup code zeroes .bss itself.
 */

ENTRY(_start)
/* Treat `_start` as referenced even though nothing calls it, so the linker
 * pulls it out of the pi5-hal rlib and --gc-sections does not throw it away. */
EXTERN(_start)

/* PROVIDE: the default, unless the build passes
 * `-C link-arg=--defsym=__load_address=...` (the chainloader is linked at
 * 0x2000000 so that programs it receives can be loaded at 0x80000). */
PROVIDE(__load_address = 0x80000);
__stack_size   = 64K;

SECTIONS
{
    . = __load_address;
    __image_start = .;

    .text : {
        /* The entry point goes first: byte 0 of kernel8.img. */
        KEEP(*(.text._start))
        *(.text .text.*)
    }

    /* AArch64 exception vector table. VBAR_ELx requires 2 KiB alignment
     * (the low 11 bits of VBAR are RES0). Its own output section keeps the
     * padding out of the middle of .text. */
    .vectors ALIGN(2048) : {
        KEEP(*(.vectors))
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    /* .bss = zero-initialised statics. Not stored in the file; the startup
     * code clears [__bss_start, __bss_end) with 8-byte stores, hence the
     * 16-byte alignment of both ends. */
    .bss (NOLOAD) : {
        . = ALIGN(16);
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    /* The stack lives right after .bss, inside the image's own footprint, so
     * we never scribble over something the firmware left below 0x80000.
     * AArch64 stacks grow DOWN, so the startup code loads SP with
     * __stack_top. SP must stay 16-byte aligned (SCTLR_ELx.SA is on by
     * default: a misaligned SP faults on the first stack access). */
    .stack (NOLOAD) : {
        . = ALIGN(16);
        __stack_bottom = .;
        . += __stack_size;
        . = ALIGN(16);
        __stack_top = .;
    }

    __image_end = .;

    /* Things that make no sense in a flat binary. */
    /DISCARD/ : {
        *(.comment)
        *(.note .note.*)
        *(.eh_frame .eh_frame_hdr)
    }
}
