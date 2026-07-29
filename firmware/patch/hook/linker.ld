ENTRY(rackforge_hook_entry);

MEMORY
{
    HOOK_SITE (rx) : ORIGIN = 0x0800B802, LENGTH = 4
    PATCH     (rx) : ORIGIN = 0x08036B80, LENGTH = 0x9480
}

SECTIONS
{
    .hook_site :
    {
        KEEP(*(.hook_site));
    } > HOOK_SITE

    .text :
    {
        KEEP(*(.text.entry));
        *(.text .text.*);
        *(.rodata .rodata.*);
    } > PATCH

    /DISCARD/ :
    {
        *(.ARM.exidx .ARM.exidx.*);
        *(.ARM.extab .ARM.extab.*);
        *(.comment);
    }
}

ASSERT(SIZEOF(.hook_site) == 4, "hook-site branch must be exactly four bytes");
ASSERT(ADDR(.text) == 0x08036B80, "unexpected patch origin");
ASSERT((ADDR(.text) + SIZEOF(.text)) <= 0x08040000, "patch exceeds application slot");
