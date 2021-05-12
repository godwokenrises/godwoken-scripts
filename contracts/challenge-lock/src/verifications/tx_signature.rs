use crate::verifications::context::{verify_tx_context, TxContext, TxContextInput};
use core::result::Result;
use gw_types::{
    packed::{
        ChallengeLockArgs, RollupConfig, VerifyTransactionSignatureWitness,
        VerifyTransactionSignatureWitnessReader,
    },
    prelude::*,
};
use validator_utils::gw_types;
use validator_utils::signature::{check_l2_account_signature_cell, SignatureType};
use validator_utils::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        high_level::load_witness_args,
    },
    error::Error,
    kv_state::KVState,
};

/// Verify tx signature
pub fn verify_tx_signature(
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
    let kv_state = KVState::new(
        ctx.kv_state(),
        unlock_args.kv_state_proof().unpack(),
        account_count,
        None,
    );
    let scripts = ctx.scripts();
    let target = lock_args.target();
    let raw_block = unlock_args.raw_l2block();
    let tx_proof = unlock_args.tx_proof();

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
        sender_script_hash, ..
    } = verify_tx_context(input)?;

    // verify sender's script is in the input
    // the script will read the layer2 tx and check user's signature.
    check_l2_account_signature_cell(&sender_script_hash, SignatureType::Transaction)?;
    Ok(())
}
