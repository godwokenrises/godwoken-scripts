//! # Finality machenism
//!
//! https://talk.nervos.org/t/optimize-godwoken-finality-and-on-chain-cost/6739
//!
//! ## Check finality
//!
//! **IMPORTANT NOTE: Offchain and onchain checking logic must be consistent.**
//!
//! - Entity is number-based, prev_global_state.last_finalized_block_number is number-based
//!
//!   Assert entity's timepoint <= prev_global_state.last_finalized_block_number
//!
//! - Entity is number-based, prev_global_state.last_finalized_block_number is timestamp-based
//!
//!   Instead of using prev_global_state.last_finalized_block_number, we choose prev_global_state.block.count as finality standard.
//!
//!   Assert entity's timepoint <= prev_global_state.block.count - 1 + FINALITY_REQUIREMENT
//!
//! - Entity is timestamp-based, prev_global_state.last_finalized_block_number is number-based
//!
//!   Currently swtiching version from v1 to v2, the entity is sure unfinalized.
//!
//! - Entity is timestamp-based, prev_global_state.last_finalized_block_number is timestamp-based
//!
//!   Assert entity's timepoint <= prev_global_state.last_finalized_block_number

use ckb_std::{
    ckb_constants::Source,
    debug,
    high_level::{load_header, QueryIter},
};
use gw_types::core::Timepoint;
use gw_types::packed::{GlobalState, RollupConfig};
use gw_types::prelude::{Entity, Unpack};

// 7 * 24 * 60 * 60 / 16800 * 1000 = 36000
const BLOCK_INTERVAL_IN_MILLISECONDS: u64 = 36000;

pub fn is_finalized(
    rollup_config: &RollupConfig,
    prev_global_state: &GlobalState,
    timepoint: &Timepoint,
) -> bool {
    match timepoint {
        Timepoint::BlockNumber(block_number) => {
            is_block_number_finalized(rollup_config, prev_global_state, *block_number)
        }
        Timepoint::Timestamp(timestamp) => is_timestamp_finalized(prev_global_state, *timestamp),
    }
}

pub fn is_timestamp_finalized(prev_global_state: &GlobalState, timestamp: u64) -> bool {
    match Timepoint::from_full_value(prev_global_state.last_finalized_block_number().unpack()) {
        Timepoint::BlockNumber(_) => {
            debug!("[is_timestamp_finalized] switching version, prev_global_state.last_finalized_block_number is number-based");
            false
        }
        Timepoint::Timestamp(finalized) => timestamp <= finalized,
    }
}

pub fn is_block_number_finalized(
    rollup_config: &RollupConfig,
    prev_global_state: &GlobalState,
    block_number: u64,
) -> bool {
    match Timepoint::from_full_value(prev_global_state.last_finalized_block_number().unpack()) {
        Timepoint::BlockNumber(finalized) => block_number <= finalized,
        Timepoint::Timestamp(_) => {
            let finality_blocks: u64 = rollup_config.finality_blocks().unpack();
            let tip_number: u64 = prev_global_state.block().count().unpack().saturating_sub(1);
            block_number.saturating_add(finality_blocks) <= tip_number
        }
    }
}

pub fn finality_as_duration(rollup_config: &RollupConfig) -> u64 {
    let finality_blocks = rollup_config.finality_blocks().unpack();
    finality_blocks.saturating_mul(BLOCK_INTERVAL_IN_MILLISECONDS)
}

/// Obtain the max timestamp of the header-deps
pub fn obtain_max_timestamp_of_header_deps() -> Option<u64> {
    let mut buf = [0u8; 8];
    QueryIter::new(load_header, Source::HeaderDep)
        .map(|header| {
            buf.copy_from_slice(header.raw().timestamp().as_slice());
            let timestamp: u64 = u64::from_le_bytes(buf);
            timestamp
        })
        .max()
}
