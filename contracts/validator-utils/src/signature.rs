use crate::{cells::utils::search_lock_hashes, error::Error};
use ckb_std::{ckb_constants::Source, debug, error::SysError, syscalls::load_cell_data};
use gw_common::H256;

pub enum SignatureType {
    Transaction,
    Message(H256),
}

/// Check l2 account signature cell
pub fn check_l2_account_signature_cell(
    script_hash: &H256,
    sig_type: SignatureType,
) -> Result<(), Error> {
    debug!("Check l2 account signature cell");
    // search layer2 account lock cell from inputs
    for index in search_lock_hashes(&(*script_hash).into(), Source::Input) {
        match sig_type {
            SignatureType::Transaction => {
                // expected data is 32
                if let Err(SysError::LengthNotEnough(full_size)) =
                    load_cell_data(&mut [], 0, index, Source::Input)
                {
                    if full_size == 32 {
                        return Ok(());
                    }
                }
            }
            SignatureType::Message(message) => {
                // expected data is equals to owner_lock_hash(32 bytes) | message(32 bytes)
                let mut data = [0u8; 32];
                let len = load_cell_data(&mut data, 32, index, Source::Input)?;

                // skip if the data isn't 32 length
                if len != data.len() {
                    continue;
                }
                if data == message.as_slice() {
                    return Ok(());
                }
            }
        }
    }
    Err(Error::AccountLockCellNotFound)
}
