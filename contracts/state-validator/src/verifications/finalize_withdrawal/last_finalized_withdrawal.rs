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

// Special value for two cases
//   - block without withdrawal
//   - block's all withdrawals
//
// NOTE: prev finalized raw block isn't required in witness to verify transition
pub const LAST_FINALIZED_WITHDRAWAL_INDEX_ALL_WITHDRAWALS: u32 = u32::MAX;

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

    let unchecked_block_withdrawals = block_withdrawals_vec.iter();
    let next_finalized_block_number = match prev_last_finalized_index {
        LastFinalizedWithdrawalIndex::AllWithdrawals => {
            prev_finalized_block_number.saturating_add(1)
        }
        LastFinalizedWithdrawalIndex::Index(_) => prev_finalized_block_number,
    };
    debug!("next block number {}", next_finalized_block_number);

    let expected_block_withdrawals_len = post_finalized_block_number
        .saturating_sub(next_finalized_block_number)
        .saturating_add(1);
    if unchecked_block_withdrawals.len() != expected_block_withdrawals_len as usize {
        debug!("witness submitted blocks dosn't match block range");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    for (next_block_number, next_block_withdrawals) in
        (next_finalized_block_number..=post_finalized_block_number).zip(unchecked_block_withdrawals)
    {
        let raw_block = next_block_withdrawals.raw_l2block();
        let opt_range = if next_block_number == prev_finalized_block_number {
            debug!("check prev finalized block {}", next_block_number);

            generate_withdrawal_range_from_prev_finalized_index(
                prev_finalized_block_number,
                &prev_last_finalized_index,
                post_finalized_block_number,
                &post_last_finalized_index,
                &raw_block,
            )?
        } else if next_block_number == post_finalized_block_number {
            debug!("check post finalized block {}", next_block_number);

            generate_withdrawal_range_from_post_finalized_index(
                &post_last_finalized_index,
                &raw_block,
            )?
        } else {
            debug!("check middle finalized block {}", next_block_number);

            Some(WithdrawalIndexRange::All)
        };

        if let Some(range) = opt_range {
            check_block_withdrawals(next_block_number, range, &next_block_withdrawals)?;
        }
    }

    Ok(())
}

fn generate_withdrawal_range_from_prev_finalized_index(
    prev_finalized_block_number: u64,
    prev_last_finalized_index: &LastFinalizedWithdrawalIndex,
    post_finalized_block_number: u64,
    post_last_finalized_index: &LastFinalizedWithdrawalIndex,
    raw_block: &RawL2BlockReader,
) -> Result<Option<WithdrawalIndexRange>, Error> {
    let prev_finalized_index =
        WithdrawalIndex::from_last_finalized_withdrawal_index(prev_last_finalized_index, raw_block);
    let block_last_withdrawal_index = WithdrawalIndex::from_block_last_withdrawal_index(&raw_block);

    let (prev_finalized_index, block_last_withdrawal_index) =
        match (prev_finalized_index, block_last_withdrawal_index) {
            (WithdrawalIndex::NoWithdrawal, WithdrawalIndex::NoWithdrawal) => {
                // Actually, we don't need to submit this block
                debug!("no withdrawal, nothing to verify");
                return Ok(None);
            }
            (WithdrawalIndex::Index(_), WithdrawalIndex::NoWithdrawal) => {
                debug!("block WithdrawalIndex::NoWithdrawal");
                return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
            }
            (WithdrawalIndex::NoWithdrawal, WithdrawalIndex::Index(_)) => {
                debug!("prev finalized index WithdrawalIndex::NoWithdrawal");
                return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
            }
            (
                WithdrawalIndex::Index(finalized_index),
                WithdrawalIndex::Index(block_last_withdrawal_index),
            ) => (finalized_index, block_last_withdrawal_index),
        };

    if prev_finalized_index > block_last_withdrawal_index {
        debug!("prev finalized index > block last withdrawal index");
        return Err(Error::InvalidRollupFinalizeWithdrawalWitness);
    }

    if prev_finalized_index == block_last_withdrawal_index {
        debug!("prev finalized index == block last withdrawal index, nothing to verify");
        return Ok(None);
    }

    if prev_finalized_block_number != post_finalized_block_number {
        // Verify all remaind withdrawals
        let all_remaind_withdrawals = WithdrawalIndexRange::new_range_inclusive(
            prev_finalized_index.saturating_add(1),
            block_last_withdrawal_index,
        )?;

        return Ok(Some(all_remaind_withdrawals));
    }

    debug!("prev_finalized_block_number == post_finalized_block_number");

    // Check post finalized index
    let post_finalized_index = match WithdrawalIndex::from_last_finalized_withdrawal_index(
        post_last_finalized_index,
        raw_block,
    ) {
        WithdrawalIndex::NoWithdrawal => {
            // When we reach here, means prev finalized block must have withdrawals
            debug!("post finalized WithdrawalIndex::NoWithdrawal");
            return Err(Error::InvalidLastFinalizedWithdrawal);
        }
        WithdrawalIndex::Index(post_finalized_index)
            if post_finalized_index > block_last_withdrawal_index =>
        {
            debug!("post finalized index > block last withdrawal index");
            return Err(Error::InvalidLastFinalizedWithdrawal);
        }
        WithdrawalIndex::Index(post_finalized_index) => post_finalized_index,
    };

    let range = WithdrawalIndexRange::new_range_inclusive(
        prev_finalized_index.saturating_add(1),
        post_finalized_index,
    )?;

    Ok(Some(range))
}

fn generate_withdrawal_range_from_post_finalized_index(
    post_last_finalized_index: &LastFinalizedWithdrawalIndex,
    raw_block: &RawL2BlockReader,
) -> Result<Option<WithdrawalIndexRange>, Error> {
    let post_finalized_index =
        WithdrawalIndex::from_last_finalized_withdrawal_index(post_last_finalized_index, raw_block);
    let block_last_withdrawal_index = WithdrawalIndex::from_block_last_withdrawal_index(&raw_block);

    let (post_finalized_index, block_last_withdrawal_index) =
        match (post_finalized_index, block_last_withdrawal_index) {
            (WithdrawalIndex::NoWithdrawal, WithdrawalIndex::Index(_))
            | (WithdrawalIndex::Index(_), WithdrawalIndex::NoWithdrawal) => {
                debug!("uncomparable post index and post block last withdrawal index");
                return Err(Error::InvalidLastFinalizedWithdrawal);
            }
            (WithdrawalIndex::NoWithdrawal, WithdrawalIndex::NoWithdrawal) => {
                debug!("no withdrawal, nothing to verify");
                return Ok(None);
            }
            (
                WithdrawalIndex::Index(finalized_index),
                WithdrawalIndex::Index(block_last_withdrawal_index),
            ) => (finalized_index, block_last_withdrawal_index),
        };

    if post_finalized_index > block_last_withdrawal_index {
        debug!("post finalized index > block last withdrawal index");
        return Err(Error::InvalidLastFinalizedWithdrawal);
    }

    let range = if post_finalized_index < block_last_withdrawal_index {
        debug!("post finalized index < block last withdrawal index");
        WithdrawalIndexRange::new_range_inclusive(0, post_finalized_index)?
    } else {
        WithdrawalIndexRange::All
    };

    Ok(Some(range))
}

#[derive(Debug)]
enum LastFinalizedWithdrawalIndex {
    AllWithdrawals,
    Index(u32),
}

impl LastFinalizedWithdrawalIndex {
    fn from_last_finalized_withdrawal(last_finalized_withdrawal: &LastFinalizedWithdrawal) -> Self {
        let value: u32 = last_finalized_withdrawal.withdrawal_index().unpack();
        if LAST_FINALIZED_WITHDRAWAL_INDEX_ALL_WITHDRAWALS == value {
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
