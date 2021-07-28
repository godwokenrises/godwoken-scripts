// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{vec, vec::Vec};
pub use validator_utils::ckb_std;
pub use validator_utils::error;
use validator_utils::kv_state::KVState;
use crate::raw_data::BIN_BLOCK;
use validator_utils::gw_types::{packed::*, prelude::*};
use validator_utils::gw_common::state::State;

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use ckb_std::{
    debug,
    high_level::{load_script, load_tx_hash},
    ckb_types::{bytes::Bytes, prelude::*},
};

use validator_utils::error::Error;

pub fn main() -> Result<(), Error> {
    // remove below examples and write your code here
    let l2block = L2BlockReader::from_slice(&BIN_BLOCK).map_err(|_err| {
        debug!("output is not a valid l2block");
        Error::Encoding
    })?;
    let kv_state = KVState::new(
        l2block.kv_state(),
        l2block.kv_state_proof().unpack(),
        0,
        None
    );

    kv_state.calculate_root()?;

    Ok(())
}

