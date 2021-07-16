use crate::verifications::context::{verify_tx_context, TxContext, TxContextInput};
use core::result::Result;
use gw_types::{
    packed::{
        ChallengeLockArgs, RollupConfig, VerifyTransactionWitness, VerifyTransactionWitnessReader,
    },
    prelude::*,
};
use validator_utils::gw_types;
use validator_utils::{
    cells::utils::search_lock_hash,
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        debug,
        high_level::load_witness_args,
    },
    error::Error,
    kv_state::KVState,
};

/// Verify tx execution
pub fn verify_tx_execution(
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match VerifyTransactionWitnessReader::verify(&witness_args, false) {
        Ok(_) => VerifyTransactionWitness::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };
    let ctx = unlock_args.context();
    let tx = unlock_args.l2tx();
    let kv_state = KVState::new(
        ctx.kv_state().as_reader(),
        unlock_args.kv_state_proof().unpack(),
        ctx.account_count().unpack(),
        None,
    );
    let scripts = ctx.scripts();
    let raw_block = unlock_args.raw_l2block();
    let target = lock_args.target();
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
        receiver_script_hash,
        ..
    } = verify_tx_context(input)?;

    // verify backend script is in the input
    // the backend will do the post state verification
    if search_lock_hash(&receiver_script_hash.into(), Source::Input).is_none() {
        debug!(
            "verify tx execution, can't find receiver_script_hash from the input: {:?}",
            &receiver_script_hash
        );
        return Err(Error::AccountScriptCellNotFound);
    }

    Ok(())
}
