// Import from `core` instead of from `std` since we are in no-std mode
use core::{convert::TryInto, result::Result};

use validator_utils::gw_types;
use validator_utils::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        debug,
        high_level::load_script,
    },
    error::Error,
    search_cells::{
        load_rollup_config, parse_rollup_action, search_rollup_cell, search_rollup_state,
    },
};

use gw_types::{
    core::ChallengeTargetType,
    packed::{ChallengeLockArgs, ChallengeLockArgsReader, RollupActionUnion},
    prelude::*,
};

/// args: rollup_type_hash | start challenge
fn parse_lock_args() -> Result<([u8; 32], ChallengeLockArgs), Error> {
    let script = load_script()?;
    let args: Bytes = script.args().unpack();

    let mut rollup_type_hash: [u8; 32] = [0u8; 32];
    if args.len() < rollup_type_hash.len() {
        return Err(Error::InvalidArgs);
    }
    rollup_type_hash.copy_from_slice(&args[..32]);
    match ChallengeLockArgsReader::verify(&args.slice(32..), false) {
        Ok(()) => Ok((
            rollup_type_hash,
            ChallengeLockArgs::new_unchecked(args.slice(32..)),
        )),
        Err(_) => Err(Error::InvalidArgs),
    }
}

/// args:
/// * rollup_script_hash | ChallengeLockArgs
///
/// unlock paths:
/// * challenge success
///   * after CHALLENGE_MATURITY_BLOCKS, the submitter can resume rollup to running status and revert the invalid rollup states
/// * cancel challenge
///   * during the rollup halting, anyone can submit context to run verification on-chain and cancel this challenge
///   * the cancel-challenge tx must contains a verifier cell in the inputs which cell's lock script equals to the account.script
///   * the lock script of verifier cell reads the context from tx.witnesses and run verification
pub fn main() -> Result<(), Error> {
    let (rollup_script_hash, lock_args) = parse_lock_args()?;

    // check rollup cell
    let index =
        search_rollup_cell(&rollup_script_hash, Source::Output).ok_or(Error::RollupCellNotFound)?;
    let action = parse_rollup_action(index, Source::Output)?;
    match action.to_enum() {
        RollupActionUnion::RollupEnterChallenge(_) | RollupActionUnion::RollupRevert(_) => {
            // state-validator will do the verification
            return Ok(());
        }
        RollupActionUnion::RollupCancelChallenge(_) => {}
        _ => {
            debug!("unsupport action {:?}", action.to_enum());
            return Err(Error::InvalidArgs);
        }
    }

    // load rollup config
    let rollup_config = {
        let prev_global_state = search_rollup_state(&rollup_script_hash, Source::Input)?
            .ok_or(Error::RollupCellNotFound)?;
        load_rollup_config(&prev_global_state.rollup_config_hash().unpack())?
    };

    // unlock via cancel challenge
    let challenge_target = lock_args.target();
    let target_type: ChallengeTargetType = {
        let target_type: u8 = challenge_target.target_type().into();
        target_type.try_into().map_err(|_| Error::InvalidArgs)?
    };

    match target_type {
        ChallengeTargetType::TxExecution => {
            crate::verifications::tx_execution::verify_tx_execution(&rollup_config, &lock_args)?;
        }
        ChallengeTargetType::TxSignature => {
            crate::verifications::tx_signature::verify_tx_signature(&rollup_config, &lock_args)?;
        }
        ChallengeTargetType::Withdrawal => {
            crate::verifications::withdrawal::verify_withdrawal(
                &rollup_script_hash,
                &rollup_config,
                &lock_args,
            )?;
        }
    }

    Ok(())
}
