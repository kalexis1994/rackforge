#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[repr(C)]
struct MinimalVectorTable {
    initial_stack_pointer: u32,
    reset: unsafe extern "C" fn() -> !,
}

// The two values match the vector table observed in Arturia firmware 1.2.1.
// Interrupt vectors are intentionally absent: this scaffold never enables one.
#[unsafe(link_section = ".vector_table")]
#[used]
static VECTOR_TABLE: MinimalVectorTable = MinimalVectorTable {
    initial_stack_pointer: 0x2002_4000,
    reset: Reset,
};

// This is deliberately inert. It proves code generation/link placement only and
// performs no reads or writes to clocks, GPIO, USB, display, flash, or QSPI.
#[unsafe(no_mangle)]
unsafe extern "C" fn Reset() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
