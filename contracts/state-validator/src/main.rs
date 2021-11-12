//! State validator
//!
//! The main script that verifies the Rollup on-chain logic
//!

#![no_std]
#![no_main]
#![feature(lang_items)]
#![feature(alloc_error_handler)]
#![feature(panic_info_message)]
#![feature(asm)]

// define modules
mod entry;
mod types;
mod verifications;

use ckb_std::default_alloc;
pub use gw_utils::ckb_std;

ckb_std::entry!(program_entry);
default_alloc!(4 * 1024, 1024 * 1024, 64);

/// program entry
fn program_entry() -> i8 {
    // Call main function and return error code
    match entry::main() {
        Ok(_) => 0,
        Err(err) => err as i8,
    }
}
