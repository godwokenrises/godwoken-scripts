// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use crate::raw_data::BIN_BLOCK;
use alloc::{vec, vec::Vec};
use gw_state::{ckb_smt::smt::Pair, constants::GW_MAX_KV_PAIRS, kv_state::KVState};
pub use gw_utils::ckb_std;
pub use gw_utils::error;
use gw_utils::gw_common::state::State;
use gw_utils::gw_types::{packed::*, prelude::*, };

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use ckb_std::{
    ckb_types::{bytes::Bytes, prelude::*},
    debug,
    high_level::{load_script, load_tx_hash},
};

use gw_utils::error::Error;

pub fn main() -> Result<(), Error> {
    // remove below examples and write your code here
    let l2block = L2BlockReader::from_slice(&BIN_BLOCK).map_err(|_err| {
        debug!("output is not a valid l2block");
        Error::Encoding
    })?;
    let mut tree_buffer = [Pair::default(); GW_MAX_KV_PAIRS];
    let kv_state_proof: Bytes = l2block.kv_state_proof().unpack();
    let kv_state = KVState::build(
        &mut tree_buffer,
        l2block.kv_state(),
        &kv_state_proof,
        0,
        None,
    )?;

    kv_state.calculate_root()?;

    Ok(())
}
