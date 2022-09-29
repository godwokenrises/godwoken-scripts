use alloc::collections::BTreeSet;
use gw_utils::{
    cells::rollup::MAX_ROLLUP_WITNESS_SIZE,
    ckb_std::{
        ckb_constants::Source,
        debug,
        syscalls::{load_witness, SysError},
    },
    error::Error,
    gw_types::{
        packed::{
            GlobalState, LastFinalizedWithdrawal, RawL2BlockReader, ScriptVecReader,
            WithdrawalRequestReader, WitnessArgsReader,
        },
        prelude::{Builder, Entity, Pack, Reader, Unpack},
    },
};

use crate::verifications::finalize_withdrawal::last_finalized_withdrawal::LAST_FINALIZED_WITHDRAWAL_INDEX_ALL_WITHDRAWALS;

pub fn check_withdrawal_owner_lock<'a>(
    withdrawals: &[WithdrawalRequestReader<'a>],
) -> Result<(), Error> {
    if withdrawals.is_empty() {
        return Ok(());
    }

    let owner_lock_hashes = load_withdrawal_owner_lock_hash_from_last_witness()?;
    let not_found = withdrawals.iter().any(|w| {
        let lock_hash: [u8; 32] = w.raw().owner_lock_hash().unpack();
        !owner_lock_hashes.contains(&lock_hash)
    });

    if not_found {
        Err(Error::InvalidWithdrawalRequest)
    } else {
        Ok(())
    }
}

pub fn can_upgrade_to_v2(prev_global_state: &GlobalState, post_global_state: &GlobalState) -> bool {
    prev_global_state.version_u8() < 2 && post_global_state.version_u8() == 2
}

pub fn upgrade_to_v2(global_state: GlobalState, raw_l2block: &RawL2BlockReader) -> GlobalState {
    let parent_block_number = raw_l2block.number().unpack().saturating_sub(1);

    let last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
        .block_number(parent_block_number.pack())
        .withdrawal_index(LAST_FINALIZED_WITHDRAWAL_INDEX_ALL_WITHDRAWALS.pack())
        .build();

    global_state
        .as_builder()
        .last_finalized_withdrawal(last_finalized_withdrawal)
        .version(2u8.into())
        .build()
}

fn load_last_witness_index() -> Result<usize, Error> {
    let mut buf = [0u8; 1];
    let mut index = 1usize;

    loop {
        match load_witness(&mut buf, 0, index, Source::Output) {
            Ok(_) => index += 1,
            Err(SysError::IndexOutOfBound) => return Ok(index - 1),
            Err(SysError::LengthNotEnough(_)) => index += 1,
            Err(err) => return Err(err.into()),
        }
    }
}

fn load_withdrawal_owner_lock_hash_from_last_witness() -> Result<BTreeSet<[u8; 32]>, Error> {
    debug!("load withdrawal owner lock hash from last witness");

    let last_witness_index = load_last_witness_index()?;
    let mut buf = [0u8; MAX_ROLLUP_WITNESS_SIZE];
    let loaded_len = load_witness(&mut buf, 0, last_witness_index, Source::Output)?;
    debug!("loaded len: {}", loaded_len);

    let witness_args = WitnessArgsReader::from_slice(&buf[..loaded_len]).map_err(|_err| {
        debug!("witness is not a valid WitnessArgsReader");
        Error::Encoding
    })?;

    let output = witness_args.output_type().to_opt().ok_or_else(|| {
        debug!("witness output_type is none");
        Error::Encoding
    })?;

    let owner_locks = ScriptVecReader::from_slice(output.raw_data()).map_err(|_err| {
        debug!("output is not a valid ScriptVecReader");
        Error::InvalidWithdrawalRequest
    })?;

    Ok(owner_locks.iter().map(|s| s.hash()).collect())
}
