//! validator-lock
//! 
//! A lock scripts designed for validator nodes in the PoA deployment environments.
//!
//! Background:
//! The Godwoken rollup in its core design is permissionless,
//! which means anybody should be able to update the rollup
//! state without permission.
//! But in the current phase, we are still too early for it,
//! thus, we designed a PoA(proof of authority) lock: clerkb
//! to support the permission deployment.
//! In the permission deployment, we allow a bunch of
//! nodes to submit l2 blocks with l2 txs to the rollup, 
//! we called these nodes block producer.
//! However, we want to introduce even more nodes to participate
//! the validating parts, these nodes do not submit l2 blocks or
//! l2 txs to the rollup, they only do validate, and only to
//! update the rollup state when the rollup running into an invalid state.
//!
//! Design:
//! Rollup cell has a field `status` which represents the current rollup status.
//! If the status is `running`, the rollup is working normally,
//! if the status is `halting` then the rollup is in a challenge process.
//!
//! A validator updates the rollup when:
//! 1. an invalid block is found, the validator start a challenge. status: `running` -> `halting`
//! 2. an invalid challenge is found, the validator cancel that challenge. status: `halting` -> `running`
//! 3. a challenge is success, the validator revert the rollup states. status: `halting` -> `running`
//!
//! 
//! validator locks can only be unlocked when the rollup are not in the tx or rollup's status is 

// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{vec, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use ckb_std::{
    debug,
    high_level::{load_script, load_tx_hash},
    ckb_types::{bytes::Bytes, prelude::*},
};

use crate::error::Error;

pub fn main() -> Result<(), Error> {
    // remove below examples and write your code here

    let script = load_script()?;
    let args: Bytes = script.args().unpack();
    debug!("script args is {:?}", args);

    // return an error if args is invalid
    if args.is_empty() {
        return Err(Error::MyError);
    }

    let tx_hash = load_tx_hash()?;
    debug!("tx hash is {:?}", tx_hash);

    let _buf: Vec<_> = vec![0u8; 32];

    Ok(())
}

