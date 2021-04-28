// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{vec, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source,
    ckb_types::{bytes::Bytes, prelude::*},
    debug,
    high_level::{load_script, load_tx_hash},
    syscalls::load_cell_data,
};
use validator_utils::{search_cells::search_lock_hash, signature::SignatureType};

use crate::error::Error;

/// Eth account lock
/// script args: rollup_script_hash(32 bytes) | eth_address(20 bytes)
/// data: owner_lock_hash(32 bytes) | *message (optional 32 bytes)
pub fn main() -> Result<(), Error> {
    // parse args
    let script = load_script()?;
    let args: Bytes = script.args().unpack();
    if args.len() != 52 {
        debug!("Invalid args len: {}", args.len());
        return Err(Error::InvalidArgs);
    }
    let rollup_script_hash = &args[..32];
    let eth_address = &args[32..];
    debug!("script args is {:?}", args);

    // parse data
    let (owner_lock_hash, sig_type) = parse_data()?;

    // check owner lock hash cell
    // to prevent others unlock this cell
    if search_lock_hash(&owner_lock_hash, Source::Input).is_none() {
        return Err(Error::OwnerLockCellNotFound);
    }

    // verify signature
    match sig_type {
        SignatureType::Transaction => {
            debug!("Verify tx signature");
            verify_tx_signature()?;
        }
        SignatureType::Message(msg) => {
            debug!("Verify message signature {:?}", msg);
            verify_message_signature()?;
        }
    }

    Ok(())
}

// parse cell's data
fn parse_data() -> Result<([u8; 32], SignatureType), Error> {
    let mut data = [0u8; 64];
    let loaded_size = load_cell_data(&mut data, 0, 0, Source::GroupInput)?;

    let sig_type = if loaded_size == 32 {
        SignatureType::Transaction
    } else if loaded_size == 64 {
        let mut msg = [0u8; 32];
        msg.copy_from_slice(&data[32..]);
        SignatureType::Message(msg.into())
    } else {
        debug!("Invalid data size: {}", loaded_size);
        return Err(Error::Encoding);
    };

    let mut owner_lock_hash = [0u8; 32];
    owner_lock_hash.copy_from_slice(&data[..32]);

    Ok((owner_lock_hash, sig_type))
}

fn verify_tx_signature() -> Result<(), Error> {
    // load signature
    // compute signing message
    // recover pubkey
    // compare with eth address
    Ok(())
}
fn verify_message_signature() -> Result<(), Error> {
    // load signature
    // recover pubkey
    // compare with eth address
    Ok(())
}
