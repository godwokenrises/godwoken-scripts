use core::result::Result;
use gw_common::{blake2b::new_blake2b, H256};
use gw_types::{
    packed::{
        ChallengeLockArgs, RawWithdrawalRequest, VerifyWithdrawalWitness,
        VerifyWithdrawalWitnessReader,
    },
    prelude::*,
};
use gw_utils::gw_common::{
    self,
    merkle_utils::{ckb_merkle_leaf_hash, CBMTMerkleProof},
};
use gw_utils::gw_types;
use gw_utils::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        debug,
        high_level::load_witness_args,
    },
    error::Error,
    signature::check_l2_account_signature_cell,
};

struct WithdrawalContext {
    raw_withdrawal: RawWithdrawalRequest,
    sender_script_hash: H256,
}

fn verify_withdrawal_proof(lock_args: &ChallengeLockArgs) -> Result<WithdrawalContext, Error> {
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match VerifyWithdrawalWitnessReader::verify(&witness_args, false) {
        Ok(_) => VerifyWithdrawalWitness::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let withdrawal = unlock_args.withdrawal_request();
    let raw_withdrawal = withdrawal.raw();
    let sender_script_hash = raw_withdrawal.account_script_hash().unpack();

    // verify block hash
    let raw_block = unlock_args.raw_l2block();
    if raw_block.hash() != lock_args.target().block_hash().as_slice() {
        debug!(
            "Wrong challenged block_hash, block_hash: {:?}, target block hash: {:?}",
            raw_block.hash(),
            lock_args.target().block_hash()
        );
        return Err(Error::InvalidBlock);
    }

    // verify withdrawal merkle proof
    let withdrawal_witness_root = raw_block
        .submit_withdrawals()
        .withdrawal_witness_root()
        .unpack();
    let withdrawal_index: u32 = lock_args.target().target_index().unpack();
    let withdrawal_witness_hash = withdrawal.witness_hash().into();
    let withdrawal_proof = unlock_args.withdrawal_proof();
    let proof = CBMTMerkleProof::new(
        withdrawal_proof.indices().unpack(),
        withdrawal_proof.lemmas().unpack(),
    );
    let hash = ckb_merkle_leaf_hash(withdrawal_index, &withdrawal_witness_hash);
    let valid = proof.verify(&withdrawal_witness_root, &[hash]);
    if !valid {
        debug!("[verify withdrawal exist] merkle verify error");
        return Err(Error::MerkleProof);
    }

    let context = WithdrawalContext {
        raw_withdrawal,
        sender_script_hash,
    };

    Ok(context)
}

fn calc_withdrawal_message(
    rollup_script_hash: &[u8; 32],
    raw_withdrawal: &RawWithdrawalRequest,
) -> [u8; 32] {
    let mut hasher = new_blake2b();
    hasher.update(rollup_script_hash);
    hasher.update(raw_withdrawal.as_slice());
    let mut message = [0u8; 32];
    hasher.finalize(&mut message);
    message
}

/// Verify withdrawal signature
pub fn verify_withdrawal(
    rollup_script_hash: &[u8; 32],
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let WithdrawalContext {
        raw_withdrawal,
        sender_script_hash,
    } = verify_withdrawal_proof(lock_args)?;

    // verify withdrawal signature
    let message = calc_withdrawal_message(rollup_script_hash, &raw_withdrawal);
    // verify sender's script is in the input
    check_l2_account_signature_cell(&sender_script_hash, message.into())?;
    Ok(())
}
