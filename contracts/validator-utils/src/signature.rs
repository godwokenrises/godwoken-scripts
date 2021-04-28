use crate::{cells::utils::search_lock_hashes, error::Error};
use ckb_std::{ckb_constants::Source, syscalls::load_cell_data};
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
    // search layer2 account lock cell from inputs
    for index in search_lock_hashes(&(*script_hash).into(), Source::Input) {
        match sig_type {
            SignatureType::Transaction => {
                // expected data is 0
                let len = load_cell_data(&mut [], 0, index, Source::Input)?;
                if len == 0 {
                    return Ok(());
                }
            }
            SignatureType::Message(message) => {
                // expected data is equals to message
                let mut data = [0u8; 32];
                let len = load_cell_data(&mut data, 0, index, Source::Input)?;

                // skip if the data isn't 32 length
                if len != data.len() {
                    continue;
                }
                if &data == message.as_slice() {
                    return Ok(());
                }
            }
        }
    }
    Err(Error::AccountLockCellNotFound)
}
