use crate::verifications::context::{verify_tx_context, TxContext, TxContextInput};
use core::result::Result;
use gw_state::{ckb_smt::smt::Pair, constants::GW_MAX_KV_PAIRS, kv_state::KVState};
use gw_types::{
    packed::{
        ChallengeLockArgs, RollupConfig, VerifyTransactionSignatureWitness,
        VerifyTransactionSignatureWitnessReader,
    },
    prelude::*,
};
use gw_utils::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        high_level::load_witness_args,
    },
    error::Error,
    signature::check_l2_account_signature_cell,
};
use gw_utils::{
    gw_common::{blake2b::new_blake2b, H256},
    gw_types::{self, packed::RawL2Transaction},
};

fn calc_tx_message(
    raw_tx: RawL2Transaction,
    rollup_type_script_hash: &[u8; 32],
    sender_script_hash: &H256,
    receiver_script_hash: &H256,
) -> H256 {
    gw_utils::ckb_std::debug!(
        "rollup: {:?} sender: {:?} receiver: {:?}",
        rollup_type_script_hash,
        sender_script_hash,
        receiver_script_hash
    );
    let mut hasher = new_blake2b();
    hasher.update(rollup_type_script_hash);
    hasher.update(sender_script_hash.as_slice());
    hasher.update(receiver_script_hash.as_slice());
    hasher.update(raw_tx.as_slice());
    let mut message = [0u8; 32];
    hasher.finalize(&mut message);
    message.into()
}

/// Verify tx signature
pub fn verify_tx_signature(
    rollup_script_hash: &[u8; 32],
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match VerifyTransactionSignatureWitnessReader::verify(&witness_args, false) {
        Ok(_) => VerifyTransactionSignatureWitness::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };
    let ctx = unlock_args.context();
    let tx = unlock_args.l2tx();
    let account_count: u32 = ctx.account_count().unpack();
    let mut tree_buffer = [Pair::default(); GW_MAX_KV_PAIRS];
    let kv_state_proof: Bytes = unlock_args.kv_state_proof().unpack();
    let kv_state = KVState::build(
        &mut tree_buffer,
        ctx.kv_state().as_reader(),
        &kv_state_proof,
        account_count,
        None,
    )?;
    let scripts = ctx.scripts();
    let target = lock_args.target();
    let raw_block = unlock_args.raw_l2block();
    let tx_proof = unlock_args.tx_proof();
    let raw_tx = tx.raw();

    let input = TxContextInput {
        tx,
        kv_state,
        scripts,
        raw_block,
        rollup_config,
        target,
        tx_proof,
    };

    let TxContext {
        sender_script_hash,
        receiver_script_hash,
    } = verify_tx_context(input)?;

    let message = calc_tx_message(
        raw_tx,
        rollup_script_hash,
        &sender_script_hash,
        &receiver_script_hash,
    );

    // verify sender's script is in the input
    check_l2_account_signature_cell(&sender_script_hash, message)?;
    Ok(())
}
