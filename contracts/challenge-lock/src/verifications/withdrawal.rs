use alloc::vec;
use core::result::Result;
use gw_common::{
    blake2b::new_blake2b,
    h256_ext::H256Ext,
    merkle_utils::calculate_state_checkpoint,
    smt::{Blake2bHasher, CompiledMerkleProof},
    state::State,
    H256,
};
use gw_types::{
    core::ScriptHashType,
    packed::{
        ChallengeLockArgs, RawWithdrawalRequest, RollupConfig, VerifyWithdrawalWitness,
        VerifyWithdrawalWitnessReader,
    },
    prelude::*,
};
use validator_utils::gw_common;
use validator_utils::gw_types;
use validator_utils::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        debug,
        high_level::load_witness_args,
    },
    error::Error,
    kv_state::KVState,
    signature::{check_l2_account_signature_cell, SignatureType},
};

struct WithdrawalContext {
    raw_withdrawal: RawWithdrawalRequest,
    sender_script_hash: H256,
}

fn verify_withdrawal_proof(
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<WithdrawalContext, Error> {
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match VerifyWithdrawalWitnessReader::verify(&witness_args, false) {
        Ok(_) => VerifyWithdrawalWitness::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let ctx = unlock_args.context();
    let scripts = ctx.scripts();

    let withdrawal = unlock_args.withdrawal_request();
    let raw_withdrawal = withdrawal.raw();
    let sender_script_hash = raw_withdrawal.account_script_hash().unpack();

    let kv_state = KVState::new(
        ctx.kv_state(),
        unlock_args.kv_state_proof().unpack(),
        ctx.account_count().unpack(),
        None,
    );

    // withdrawal nonce
    let nonce: u32 = raw_withdrawal.nonce().unpack();
    let sender_id = kv_state
        .get_account_id_by_script_hash(&sender_script_hash)?
        .ok_or(Error::AccountNotFound)?;
    let sender_nonce = kv_state.get_nonce(sender_id)?;
    if nonce != sender_nonce {
        debug!(
            "invalid nonce, withdrawal.nonce {} sender.nonce {}",
            nonce, sender_nonce
        );
        return Err(Error::UnexpectedTxNonce);
    }

    // find sender script
    let sender_script = scripts
        .into_iter()
        .find(|script| H256::from(script.hash()) == sender_script_hash)
        .ok_or(Error::ScriptNotFound)?;

    // withdrawal account must be a valid External Owned Account
    if sender_script.hash_type() != ScriptHashType::Type.into() {
        debug!("Invalid sender script hash type: Data");
        return Err(Error::UnknownEOAScript);
    }
    if rollup_config
        .allowed_eoa_type_hashes()
        .into_iter()
        .find(|code_hash| code_hash == &sender_script.code_hash())
        .is_none()
    {
        debug!("Unknown sender code hash: {:?}", sender_script.code_hash());
        return Err(Error::UnknownEOAScript);
    }

    // verify block hash
    let raw_block = unlock_args.raw_l2block();
    if raw_block.hash() != lock_args.target().block_hash().as_slice() {
        debug!(
            "Wrong challenged block_hash, block_hash: {:?}, target block hash: {:?}",
            raw_block.hash(),
            lock_args.target().block_hash()
        );
        return Err(Error::InvalidOutput);
    }

    // verify withdrawal merkle proof
    let withdrawal_witness_root: [u8; 32] = raw_block
        .submit_withdrawals()
        .withdrawal_witness_root()
        .unpack();
    let withdrawal_index: u32 = lock_args.target().target_index().unpack();
    let withdrawal_witness_hash: [u8; 32] = withdrawal.witness_hash();
    let valid = CompiledMerkleProof(unlock_args.withdrawal_proof().unpack())
        .verify::<Blake2bHasher>(
            &withdrawal_witness_root.into(),
            vec![(
                H256::from_u32(withdrawal_index),
                withdrawal_witness_hash.into(),
            )],
        )
        .map_err(|_err| {
            debug!("withdrawal_witness_root merkle proof error: {:?}", _err);
            Error::MerkleProof
        })?;
    if !valid {
        debug!("Wrong withdrawal merkle proof");
        return Err(Error::MerkleProof);
    }

    // verify kv-state merkle proof (prev state root)
    let prev_state_checkpoint: H256 = match withdrawal_index.checked_sub(1) {
        Some(prev_checkpoint_index) => raw_block
            .state_checkpoint_list()
            .get(prev_checkpoint_index as usize)
            .ok_or(Error::InvalidStateCheckpoint)?
            .unpack(),
        None => {
            let prev_account = raw_block.prev_account();
            calculate_state_checkpoint(
                &prev_account.merkle_root().unpack(),
                prev_account.count().unpack(),
            )
        }
    };

    let state_root = kv_state.calculate_root().map_err(|_err| {
        debug!(
            "verify_withdrawal kv_state calculate_root error: {:?}",
            _err
        );
        Error::MerkleProof
    })?;
    let account_count = kv_state.get_account_count()?;
    let calculated_prev_state_checkpoint: H256 =
        calculate_state_checkpoint(&state_root, account_count);
    if prev_state_checkpoint != calculated_prev_state_checkpoint {
        debug!("verify_withdrawal mismatch prev_state_checkpoint: {:?}, calculated_prev_state_checkpoint: {:?}", prev_state_checkpoint, calculated_prev_state_checkpoint);
        return Err(Error::MerkleProof);
    }

    let context = WithdrawalContext {
        raw_withdrawal,
        sender_script_hash,
    };

    Ok(context)
}

/// Verify withdrawal signature
pub fn verify_withdrawal(
    rollup_script_hash: &[u8; 32],
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let WithdrawalContext {
        raw_withdrawal,
        sender_script_hash,
    } = verify_withdrawal_proof(rollup_config, lock_args)?;

    // verify withdrawal signature
    let message = {
        let mut hasher = new_blake2b();
        hasher.update(rollup_script_hash);
        hasher.update(raw_withdrawal.as_slice());
        let mut message = [0u8; 32];
        hasher.finalize(&mut message);
        message.into()
    };

    // verify sender's script is in the input
    // the script will check user's signature.
    check_l2_account_signature_cell(&sender_script_hash, SignatureType::Message(message))?;

    Ok(())
}
