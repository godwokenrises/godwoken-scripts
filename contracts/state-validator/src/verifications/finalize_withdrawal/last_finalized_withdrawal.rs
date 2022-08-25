use core::{ops::RangeInclusive, result::Result};

use alloc::vec::Vec;
use gw_utils::{
    ckb_std::debug,
    error::Error,
    gw_common::{
        merkle_utils::{ckb_merkle_leaf_hash, CBMTMerkleProof},
        H256,
    },
    gw_types::{
        packed::{
            LastFinalizedWithdrawal, RawL2BlockReader, RawL2BlockWithdrawalsReader,
            RawL2BlockWithdrawalsVecReader,
        },
        prelude::Unpack,
    },
};

pub const BLOCK_WITHDRAWAL_INDEX_NO_WITHDRAWAL: u32 = u32::MAX;
// Use this value, we don't need to submit prev block witness if all withdrawals are finalized
pub const BLOCK_WITHDRAWAL_INDEX_ALL_WITHDRAWALS: u32 = u32::MAX - 1;

#[must_use]
pub fn check(
    last_finalized_block_number: u64,
    block_withdrawals_vec: &RawL2BlockWithdrawalsVecReader,
    prev_last_finalized_withdrawal: LastFinalizedWithdrawal,
    post_last_finalized_withdrawal: LastFinalizedWithdrawal,
) -> Result<(), Error> {
    debug!("check last_finalized_withdrawal");

    if block_withdrawals_vec.is_empty() {
        debug!("witness doens't have blocks");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    let prev_finalized_block_number = prev_last_finalized_withdrawal.block_number().unpack();
    let post_finalized_block_number = post_last_finalized_withdrawal.block_number().unpack();
    debug!("prev finalized block {}", prev_finalized_block_number);
    debug!("post finalized block {}", post_finalized_block_number);

    if post_finalized_block_number < prev_finalized_block_number {
        debug!("post block number < prev block number");
        return Err(Error::InvalidLastFinalizedWithdrawal);
    }
    if post_finalized_block_number > last_finalized_block_number {
        debug!("post block number  > last finalized block number");
        return Err(Error::InvalidLastFinalizedWithdrawal);
    }

    let prev_last_finalized_index = LastFinalizedWithdrawalIndex::from_last_finalized_withdrawal(
        &prev_last_finalized_withdrawal,
    );
    let post_last_finalized_index = LastFinalizedWithdrawalIndex::from_last_finalized_withdrawal(
        &post_last_finalized_withdrawal,
    );
    debug!("prev finalized idx {:?}", prev_last_finalized_index);
    debug!("post finalized idx {:?}", post_last_finalized_index);

    // Same block rule:
    // 1. post index must not be LastFinalizedWithdrawalIndex::NoWithdrawal
    // 2. post index must be greater than prev index
    // 3. witness block match block number
    // 4. witness withdrawals match index range len
    // 5. witness withdrawals have valid cbmt merkle proof
    if post_finalized_block_number == prev_finalized_block_number {
        debug!("finalize withdrawal from same block");

        if 1 != block_withdrawals_vec.len() {
            debug!("witness submit extra block withdrawals");
            return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
        }

        if matches!(
            post_last_finalized_index,
            LastFinalizedWithdrawalIndex::NoWithdrawal
        ) {
            debug!("post index == LastFinalizedWithdrawalIndex::NoWithdrawal");
            return Err(Error::InvalidLastFinalizedWithdrawal);
        }

        let block_withdrawals = block_withdrawals_vec.get_unchecked(0);
        let prev_finalized_index = WithdrawalIndex::from_last_finalized_withdrawal_index(
            &prev_last_finalized_index,
            &block_withdrawals.raw_l2block(),
        );
        let post_finalized_index = WithdrawalIndex::from_last_finalized_withdrawal_index(
            &post_last_finalized_index,
            &block_withdrawals.raw_l2block(),
        );

        let withdrawals_index_range = match (prev_finalized_index, post_finalized_index) {
            (WithdrawalIndex::NoWithdrawal, _) | (_, WithdrawalIndex::NoWithdrawal) => {
                debug!("index == WithdrawalIndex::NoWithdrawal");
                return Err(Error::InvalidLastFinalizedWithdrawal);
            }
            (WithdrawalIndex::Index(prev_idx), WithdrawalIndex::Index(post_idx)) => {
                if post_idx <= prev_idx {
                    debug!("post index <= prev index");
                    return Err(Error::InvalidLastFinalizedWithdrawal);
                }

                WithdrawalIndexRange::new_range_inclusive(prev_idx.saturating_add(1), post_idx)?
            }
        };

        let block_number = block_withdrawals.raw_l2block().number().unpack();
        if block_number != post_finalized_block_number {
            debug!("witness wrong block number");
            return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
        }

        check_block_withdrawals(block_number, withdrawals_index_range, &block_withdrawals)?;
    }
    // post_finalized_block_number > prev_finalized_block_number
    //
    // Across block rule:
    // 1. check whether all withdrawals from prev block are finalized
    // 2. post last index must be either LastFinalizedWithdrawalIndex::NoWithdrawal,
    //    LastFinalizedWithdrawalIndex::AllWithdrawals or within post block last withdrawal index range
    // 3. check prev block +1 ..= post block finalize
    else {
        debug!("finalize across blocks");

        let mut unchecked_block_withdrawals = block_withdrawals_vec.iter();

        // Check whether all withdrawals in prev block are finalized
        let may_have_unfinalized = matches!(
            prev_last_finalized_index,
            LastFinalizedWithdrawalIndex::Index(_)
        );
        if may_have_unfinalized {
            debug!("check prev block finalize status");

            let prev_block_withdrawals = match unchecked_block_withdrawals.next() {
                Some(prev) => prev,
                None => {
                    debug!("witness no prev block");
                    return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
                }
            };
            if prev_block_withdrawals.raw_l2block().number().unpack() != prev_finalized_block_number
            {
                debug!("witness wrong prev block number");
                return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
            }

            let prev_finalized_index = WithdrawalIndex::from_last_finalized_withdrawal_index(
                &prev_last_finalized_index,
                &prev_block_withdrawals.raw_l2block(),
            );
            let prev_block_last_withdrawal_index =
                WithdrawalIndex::from_block_last_withdrawal_index(
                    &prev_block_withdrawals.raw_l2block(),
                );

            let has_unfinalized = match (prev_finalized_index, prev_block_last_withdrawal_index) {
                (WithdrawalIndex::NoWithdrawal, _) => {
                    debug!("unreachable prev index WithdrawalIndex::NoWithdrawal");
                    return Err(Error::InvalidLastFinalizedWithdrawal);
                }
                (_, WithdrawalIndex::NoWithdrawal) => {
                    debug!("prev block index WithdrawalIndex::NoWithdrawal");
                    return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
                }
                (
                    WithdrawalIndex::Index(prev_finalized_index),
                    WithdrawalIndex::Index(block_last_index),
                ) => {
                    if prev_finalized_index > block_last_index {
                        debug!("prev index > witness prev block last withdrawal index");
                        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
                    }

                    if prev_finalized_index < block_last_index {
                        let range = WithdrawalIndexRange::new_range_inclusive(
                            prev_finalized_index.saturating_add(1),
                            block_last_index,
                        )?;
                        Some(range)
                    } else {
                        None // prev_finalized_index == block_last_index
                    }
                }
            };

            if let Some(withdrawals_range) = has_unfinalized {
                check_block_withdrawals(
                    prev_finalized_block_number,
                    withdrawals_range,
                    &prev_block_withdrawals,
                )?;
            }
        }

        // Check post withdrawal index
        let post_block_withdrawals = {
            let post_index = block_withdrawals_vec.len().saturating_sub(1);
            block_withdrawals_vec.get_unchecked(post_index)
        };
        let post_finalized_index = WithdrawalIndex::from_last_finalized_withdrawal_index(
            &post_last_finalized_index,
            &post_block_withdrawals.raw_l2block(),
        );
        let post_block_last_withdrawal_index = WithdrawalIndex::from_block_last_withdrawal_index(
            &post_block_withdrawals.raw_l2block(),
        );

        let post_block_finalized_range =
            match (post_finalized_index, post_block_last_withdrawal_index) {
                (WithdrawalIndex::NoWithdrawal, WithdrawalIndex::Index(_))
                | (WithdrawalIndex::Index(_), WithdrawalIndex::NoWithdrawal) => {
                    debug!("uncomparable post index and post block last withdrawal index");
                    return Err(Error::InvalidLastFinalizedWithdrawal);
                }
                (WithdrawalIndex::NoWithdrawal, WithdrawalIndex::NoWithdrawal) => {
                    WithdrawalIndexRange::All
                }
                (
                    WithdrawalIndex::Index(post_finalized_index),
                    WithdrawalIndex::Index(post_block_last_index),
                ) => {
                    if post_finalized_index > post_block_last_index {
                        debug!("post index > post block last withdrawal index");
                        return Err(Error::InvalidLastFinalizedWithdrawal);
                    }

                    if post_finalized_index < post_block_last_index {
                        debug!("post index < post block last withdrawal index");

                        WithdrawalIndexRange::new_range_inclusive(0, post_finalized_index)?
                    } else {
                        WithdrawalIndexRange::All
                    }
                }
            };

        // Check reset of blocks
        let next_finalized_block_number = prev_finalized_block_number.saturating_add(1);
        let remained_blocks_len = post_finalized_block_number
            .saturating_sub(next_finalized_block_number)
            .saturating_add(1);
        if unchecked_block_withdrawals.len() != remained_blocks_len as usize {
            debug!("witness submitted blocks dosn't match block range");
            return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
        }

        for (next_block_number, next_block_withdrawals) in (next_finalized_block_number
            ..=post_finalized_block_number)
            .zip(unchecked_block_withdrawals)
        {
            if next_block_number != post_finalized_block_number {
                check_block_withdrawals(
                    next_block_number,
                    WithdrawalIndexRange::All,
                    &next_block_withdrawals,
                )?;
            } else {
                check_block_withdrawals(
                    next_block_number,
                    post_block_finalized_range.clone(),
                    &next_block_withdrawals,
                )?;
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
enum LastFinalizedWithdrawalIndex {
    NoWithdrawal,
    AllWithdrawals,
    Index(u32),
}

impl LastFinalizedWithdrawalIndex {
    fn from_last_finalized_withdrawal(last_finalized_withdrawal: &LastFinalizedWithdrawal) -> Self {
        let value: u32 = last_finalized_withdrawal.withdrawal_index().unpack();
        if BLOCK_WITHDRAWAL_INDEX_NO_WITHDRAWAL == value {
            LastFinalizedWithdrawalIndex::NoWithdrawal
        } else if BLOCK_WITHDRAWAL_INDEX_ALL_WITHDRAWALS == value {
            LastFinalizedWithdrawalIndex::AllWithdrawals
        } else {
            LastFinalizedWithdrawalIndex::Index(value)
        }
    }
}

#[derive(Debug, PartialEq)]
enum WithdrawalIndex {
    NoWithdrawal,
    Index(u32),
}

impl WithdrawalIndex {
    fn from_block_last_withdrawal_index(raw_block: &RawL2BlockReader) -> Self {
        let count: u32 = raw_block.submit_withdrawals().withdrawal_count().unpack();
        if 0 == count {
            WithdrawalIndex::NoWithdrawal
        } else {
            WithdrawalIndex::Index(count.saturating_sub(1))
        }
    }

    fn from_last_finalized_withdrawal_index(
        index: &LastFinalizedWithdrawalIndex,
        raw_block: &RawL2BlockReader,
    ) -> Self {
        use LastFinalizedWithdrawalIndex::*;

        match index {
            NoWithdrawal => WithdrawalIndex::NoWithdrawal,
            AllWithdrawals => WithdrawalIndex::from_block_last_withdrawal_index(raw_block),
            Index(val) => WithdrawalIndex::Index(*val),
        }
    }
}

#[derive(Clone)]
enum WithdrawalIndexRange {
    All,
    RangeInclusive(RangeInclusive<u32>),
}

impl WithdrawalIndexRange {
    fn new_range_inclusive(start: u32, end: u32) -> Result<Self, Error> {
        if start > end {
            debug!("invalid range {:?} > {:?}", start, end);
            return Err(Error::InvalidLastFinalizedWithdrawal);
        }

        let range = RangeInclusive::new(start, end);
        Ok(WithdrawalIndexRange::RangeInclusive(range))
    }
}

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

#[must_use]
fn check_inclusive_range_withrawals(
    block_withdrawals: &RawL2BlockWithdrawalsReader,
    range_inclusive: RangeInclusive<u32>,
) -> Result<(), Error> {
    debug!(
        "check from {} to {}",
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
