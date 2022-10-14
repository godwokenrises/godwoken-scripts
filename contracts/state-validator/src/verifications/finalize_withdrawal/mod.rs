use core::result::Result;

use gw_state::ckb_smt::smt::{Pair, Tree};
use gw_utils::{
    ckb_std::debug,
    error::Error,
    gw_common::H256,
    gw_types::{
        bytes::Bytes,
        packed::{GlobalState, RawL2Block, RollupConfig, RollupFinalizeWithdrawalReader},
        prelude::{Builder, Entity, Unpack},
    },
};

mod types;
mod user_withdrawal_cells;
pub mod withdrawal_cursor;

#[must_use]
pub fn verify(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    args: RollupFinalizeWithdrawalReader,
    prev_global_state: GlobalState,
    post_global_state: GlobalState,
) -> Result<(), Error> {
    debug!("verify finalized withdrawal");

    // Check global state version is 2
    if 2 != prev_global_state.version_u8() || 2 != post_global_state.version_u8() {
        debug!("global state invalid version");
        return Err(Error::InvalidGlobalStateVersion);
    }

    // Check witness block proof
    check_block_proof(&prev_global_state, &args)?;

    // Check global state `last_finalized_withdrawal` and witness withdrawals proof
    let last_finalized_block_number = prev_global_state.last_finalized_block_number().unpack();
    withdrawal_cursor::check(
        last_finalized_block_number,
        &args.block_withdrawals(),
        prev_global_state
            .finalized_withdrawal_cursor()
            .unpack_cursor(),
        post_global_state
            .finalized_withdrawal_cursor()
            .unpack_cursor(),
    )?;

    // Check input/output custodian cells and output user withdrawal cells
    user_withdrawal_cells::check(
        rollup_type_hash,
        config,
        last_finalized_block_number,
        &args.block_withdrawals(),
    )?;

    // Check global state, must only update `last_finalized_withdrawal`
    let expected_post_global_state = prev_global_state
        .as_builder()
        .finalized_withdrawal_cursor(post_global_state.finalized_withdrawal_cursor())
        .build();
    if expected_post_global_state.as_slice() != post_global_state.as_slice() {
        debug!("global state update extra field(s)");
        return Err(Error::InvalidPostGlobalState);
    }

    Ok(())
}

#[must_use]
fn check_block_proof(
    prev_global_state: &GlobalState,
    args: &RollupFinalizeWithdrawalReader,
) -> Result<(), Error> {
    let block_root: [u8; 32] = prev_global_state.block().merkle_root().unpack();
    let block_proof: Bytes = args.block_proof().unpack();
    let block_withdrawals_vec = args.block_withdrawals();

    let mut buf = [Pair::default(); 256];
    let mut block_tree = Tree::new(&mut buf);
    let block_smt_keys_leaves = block_withdrawals_vec.iter().map(|block_withdrawals| {
        let raw_block = block_withdrawals.raw_l2block();
        let block_smt_key = RawL2Block::compute_smt_key(raw_block.number().unpack());
        (block_smt_key, raw_block.hash())
    });
    for (block_smt_key, block_hash) in block_smt_keys_leaves {
        if let Err(err) = block_tree.update(&block_smt_key, &block_hash) {
            debug!("verify block proof, update kv error {}", err);
            return Err(Error::MerkleProof);
        }
    }

    if let Err(err) = block_tree.verify(&block_root, &block_proof) {
        debug!("witness block merkle proof verify error {}", err);
        return Err(Error::MerkleProof);
    }

    Ok(())
}
