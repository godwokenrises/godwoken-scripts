use core::{ops::RangeInclusive, result::Result};

use super::types::WithdrawalIndexRange;
use alloc::vec::Vec;
use gw_utils::{
    ckb_std::debug,
    error::Error,
    gw_common::{
        merkle_utils::{ckb_merkle_leaf_hash, CBMTMerkleProof},
        H256,
    },
    gw_types::{
        core::{WithdrawalCursor, WithdrawalCursorIndex},
        packed::{RawL2BlockWithdrawalsReader, RawL2BlockWithdrawalsVecReader},
        prelude::Unpack,
    },
};

/// Check
///
/// This function verifies the finalized withdrawals
/// from prev_cursor (non-included) to post_cursor (included)
#[must_use]
pub fn check(
    last_finalized_block_number: u64,
    block_withdrawals_vec: &RawL2BlockWithdrawalsVecReader,
    prev_cursor: WithdrawalCursor,
    post_cursor: WithdrawalCursor,
) -> Result<(), Error> {
    debug!("check last_finalized_withdrawal");

    if block_withdrawals_vec.is_empty() {
        debug!("witness doens't have blocks");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }
    // check cursor

    if post_cursor.block_number < prev_cursor.block_number {
        debug!("post withdrawal block number < prev withdrawal block number");
        return Err(Error::FinalizeWithdrawal);
    }
    if post_cursor.block_number > last_finalized_block_number {
        debug!("post withdrawal block number  > last finalized block number");
        return Err(Error::FinalizeWithdrawal);
    }

    debug!(
        "finalize withdrawals from cursor {:?} to {:?}",
        prev_cursor, post_cursor
    );

    let unchecked_block_withdrawals = block_withdrawals_vec.iter();
    let start_cursor = match prev_cursor.index {
        WithdrawalCursorIndex::All => {
            // use next block's as start cursor
            let block = block_withdrawals_vec
                .get(0)
                .map(|w| w.raw_l2block())
                .filter(|b| b.number().unpack() == prev_cursor.block_number + 1)
                .ok_or_else(|| {
                    debug!("can't find start cursor's block");
                    Error::InvalidRollupFinalizeWithdrawalWitness
                })?;
            // build start cursor
            WithdrawalCursor::build_cursor(
                block.number().unpack(),
                0,
                block.submit_withdrawals().withdrawal_count().unpack(),
            )
            .ok_or(Error::InvalidRollupFinalizeWithdrawalWitness)?
        }
        WithdrawalCursorIndex::Index(index) => {
            // use next withdrawal as start cursor
            WithdrawalCursor {
                block_number: prev_cursor.block_number,
                index: WithdrawalCursorIndex::Index(index + 1),
            }
        }
    };
    debug!(
        "Check finalize withdrawal block from number {}",
        start_cursor.block_number
    );

    let expected_block_withdrawals_len = post_cursor
        .block_number
        .saturating_sub(start_cursor.block_number)
        .saturating_add(1);
    if unchecked_block_withdrawals.len() != expected_block_withdrawals_len as usize {
        debug!("unexpected block withdrawals length");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    // verify finalized withdrawals by block
    for (block_number, withdrawals) in
        (start_cursor.block_number..=post_cursor.block_number).zip(unchecked_block_withdrawals)
    {
        let raw_block = withdrawals.raw_l2block();

        debug!(
            "block {} start_cursor {:?} end_cursor {:?}",
            block_number, start_cursor, post_cursor
        );

        // build withdrawal range
        let withdrawal_range = if block_number != start_cursor.block_number
            && block_number != post_cursor.block_number
        {
            WithdrawalIndexRange::All
        } else {
            let start = if block_number == start_cursor.block_number {
                start_cursor.index
            } else {
                WithdrawalCursorIndex::Index(0)
            };

            let end = if block_number == post_cursor.block_number {
                post_cursor.index
            } else {
                WithdrawalCursorIndex::All
            };

            WithdrawalIndexRange::build_range_inclusive(&raw_block, start, end)?
        };

        debug!(
            "check finalized withdrawals block number {}, index range: {:?}",
            block_number, withdrawal_range
        );

        check_block_withdrawals(block_number, withdrawal_range, &withdrawals)?;
    }

    Ok(())
}

/// check_block_withdrawals
///
/// Verify withdrawals in a block.
///
/// # Args
///
/// - `block_number`: block's number
/// - `withdrawal_index_range`: range of withdrawals will been verified
/// - `block_withdrawals`: block withdrawal witness data
#[must_use]
fn check_block_withdrawals(
    block_number: u64,
    withdrawal_index_range: WithdrawalIndexRange,
    block_withdrawals: &RawL2BlockWithdrawalsReader,
) -> Result<(), Error> {
    debug!("check block {} withdrawals", block_number);

    if block_withdrawals.raw_l2block().number().unpack() != block_number {
        debug!("witness wrong block");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    match withdrawal_index_range {
        WithdrawalIndexRange::RangeInclusive(range) => {
            check_inclusive_range_withrawals(block_withdrawals, range)?
        }
        WithdrawalIndexRange::All => {
            let submit_withdrawals = block_withdrawals.raw_l2block().submit_withdrawals();
            let withdrawals_count: u32 = submit_withdrawals.withdrawal_count().unpack();
            let witness_has_withdrawals = !block_withdrawals.withdrawals().is_empty();
            if 0 == withdrawals_count && witness_has_withdrawals {
                debug!("witness submit withdrawals but block doens't have withdrawals");
                return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
            }

            if 0 != withdrawals_count {
                let last_withdrawal_index = withdrawals_count.saturating_sub(1);
                let range = RangeInclusive::new(0, last_withdrawal_index);
                check_inclusive_range_withrawals(block_withdrawals, range)?;
            }
        }
    }

    Ok(())
}

/// Verify a part of withdrawals in a block
#[must_use]
fn check_inclusive_range_withrawals(
    block_withdrawals: &RawL2BlockWithdrawalsReader,
    range_inclusive: RangeInclusive<u32>,
) -> Result<(), Error> {
    debug!(
        "check_inclusive_range_withrawals: from {} to {}",
        range_inclusive.start(),
        range_inclusive.end()
    );

    if range_inclusive.start() > range_inclusive.end() {
        debug!("start index > end index");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    let submit_withdrawals = block_withdrawals.raw_l2block().submit_withdrawals();
    let withdrawal_count: u32 = submit_withdrawals.withdrawal_count().unpack();
    if 0 == withdrawal_count {
        debug!("witness withdrawal count is zero");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    if range_inclusive.start() >= &withdrawal_count {
        debug!("start index >= withdrawal count");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }
    // End index for inclusive range must be less than withdrawal count
    if range_inclusive.end() >= &withdrawal_count {
        debug!("end index >= withdrawal count");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    let withdrawals = block_withdrawals.withdrawals();
    if range_inclusive.clone().count() != withdrawals.len() {
        debug!("witness withdrawals len doesn't match index range");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    // Verify merkle proof
    let withdrawal_proof = block_withdrawals.withdrawal_proof();
    let proof = CBMTMerkleProof::new(
        withdrawal_proof.indices().unpack(),
        withdrawal_proof.lemmas().unpack(),
    );

    let withdrawal_witness_root: H256 = submit_withdrawals.withdrawal_witness_root().unpack();
    let withdrawal_hashes = range_inclusive
        .zip(withdrawals.iter())
        .map(|(withdrawal_idx, withdrawal)| {
            ckb_merkle_leaf_hash(withdrawal_idx, &withdrawal.witness_hash().into())
        })
        .collect::<Vec<_>>();

    let valid = proof.verify(&withdrawal_witness_root, &withdrawal_hashes);
    if !valid {
        debug!("witness block withdrawals merkle proof verify error");
        return Err(Error::MerkleProof);
    }

    Ok(())
}
