/*
 * Provisional N32G455 memory map inferred from the official Arturia image.
 * This script is only for producing an offline ELF. It is not a DFU image.
 */
ENTRY(Reset);

MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08007800, LENGTH = 226K
    RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 144K
}

_stack_top = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
    .vector_table ORIGIN(FLASH) :
    {
        KEEP(*(.vector_table));
    } > FLASH

    .text :
    {
        *(.text .text.*);
        *(.rodata .rodata.*);
    } > FLASH

    .data :
    {
        _sdata = .;
        *(.data .data.*);
        _edata = .;
    } > RAM AT > FLASH
    _sidata = LOADADDR(.data);

    .bss (NOLOAD) :
    {
        _sbss = .;
        *(.bss .bss.*);
        *(COMMON);
        _ebss = .;
    } > RAM

    /DISCARD/ :
    {
        *(.ARM.exidx .ARM.exidx.*);
        *(.ARM.extab .ARM.extab.*);
    }
}

ASSERT(SIZEOF(.vector_table) == 8, "unexpected vector table size");
ASSERT(ORIGIN(FLASH) == 0x08007800, "wrong application base");
ASSERT(_stack_top == 0x20024000, "wrong SRAM limit");
