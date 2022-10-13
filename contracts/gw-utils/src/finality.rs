//! # Finality machenism
//!
//! https://talk.nervos.org/t/optimize-godwoken-finality-and-on-chain-cost/6739
//!
//! ## Check finality
//!
//! - post_version < 2, entity is number-based
//!   Assert `post_global_state.block.count - 1 + FINALITY_CONFIGURATION >= entity.number`
//!
//! - post_version < 2, entity is time-based
//!   Impossible
//!
//! - post_version >= 2, entity is number-based
//!   Assert `post_global_state.block.count - 1 + FINALITY_CONFIGURATION >= entity.number`
//!
//! - post_version >= 2, entity is time-based
//!   post_global_state.last_finalized_block_number must be time-based.
//!   Assert `post_global_state.last_finalized_block_number + FINALITY_CONFIGURATION >= entity.time`

use crate::error::Error;
use ckb_std::{
    ckb_constants::Source,
    high_level::{load_header, QueryIter},
};
use gw_types::core::Timepoint;
use gw_types::packed::{GlobalState, RollupConfig};
use gw_types::prelude::{Entity, Unpack};

// 7 * 24 * 60 * 60 / 16800 * 1000 = 36000
const BLOCK_INTERVAL_IN_MILLISECONDS: u64 = 36000;

pub fn is_finalized(
    rollup_config: &RollupConfig,
    global_state: &GlobalState,
    timepoint: &Timepoint,
) -> Result<bool, Error> {
    match timepoint {
        Timepoint::BlockNumber(block_number) => Ok(is_block_number_finalized(
            rollup_config,
            global_state,
            *block_number,
        )),
        Timepoint::Timestamp(timestamp) => {
            is_timestamp_finalized(rollup_config, global_state, *timestamp)
        }
    }
}

pub fn is_timestamp_finalized(
    rollup_config: &RollupConfig,
    global_state: &GlobalState,
    timestamp: u64,
) -> Result<bool, Error> {
    let finality = finality_as_duration(rollup_config);
    match Timepoint::from_full_value(global_state.last_finalized_block_number().unpack()) {
        Timepoint::BlockNumber(_) => Err(Error::InvalidPostGlobalState),
        Timepoint::Timestamp(finalized) => Ok(timestamp <= finalized.saturating_add(finality)),
    }
}

pub fn is_block_number_finalized(
    rollup_config: &RollupConfig,
    global_state: &GlobalState,
    block_number: u64,
) -> bool {
    let finality = finality_as_blocks(rollup_config);
    let tip_number: u64 = global_state.block().count().unpack().saturating_sub(1);
    block_number.saturating_add(finality) <= tip_number
}

pub fn finality_as_duration(rollup_config: &RollupConfig) -> u64 {
    match Timepoint::from_full_value(rollup_config.finality_blocks().unpack()) {
        Timepoint::BlockNumber(block_number) => {
            block_number.saturating_mul(BLOCK_INTERVAL_IN_MILLISECONDS)
        }
        Timepoint::Timestamp(timestamp) => timestamp,
    }
}

pub fn finality_as_blocks(rollup_config: &RollupConfig) -> u64 {
    match Timepoint::from_full_value(rollup_config.finality_blocks().unpack()) {
        Timepoint::BlockNumber(block_number) => block_number,
        Timepoint::Timestamp(timestamp) => timestamp / BLOCK_INTERVAL_IN_MILLISECONDS,
    }
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
