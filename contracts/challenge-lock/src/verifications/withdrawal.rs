use crate::verifications::eip712::traits::EIP712Encode;
use alloc::string::ToString;
use core::result::Result;
use gw_common::H256;
use gw_types::{packed::ChallengeLockArgs, prelude::*};
use gw_utils::gw_types::{
    self,
    packed::{RollupConfig, Script, WithdrawalRequest},
};
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
use gw_utils::{
    gw_common::{
        self,
        merkle_utils::{ckb_merkle_leaf_hash, CBMTMerkleProof},
    },
    gw_types::packed::{CCWithdrawalWitness, CCWithdrawalWitnessReader},
};

use super::eip712::types::EIP712Domain;

struct WithdrawalContext {
    withdrawal: WithdrawalRequest,
    sender_script_hash: H256,
    owner_lock: Script,
}

fn verify_withdrawal_proof(lock_args: &ChallengeLockArgs) -> Result<WithdrawalContext, Error> {
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match CCWithdrawalWitnessReader::verify(&witness_args, false) {
        Ok(_) => CCWithdrawalWitness::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let withdrawal = unlock_args.withdrawal();
    let raw_withdrawal = withdrawal.raw();
    let sender_script_hash = raw_withdrawal.account_script_hash().unpack();
    let sender = unlock_args.sender();
    let owner_lock = unlock_args.owner_lock();

    if H256::from(sender.hash()) != sender_script_hash {
        debug!("Mismatch sender script hash");
        return Err(Error::InvalidArgs);
    }

    if H256::from(owner_lock.hash()) != raw_withdrawal.owner_lock_hash().unpack() {
        debug!("Mismatch owner lock hash");
        return Err(Error::InvalidArgs);
    }

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
        withdrawal,
        sender_script_hash,
        owner_lock,
    };

    Ok(context)
}

/// Verify withdrawal signature
pub fn verify_withdrawal(
    _rollup_script_hash: &[u8; 32],
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let WithdrawalContext {
        withdrawal,
        sender_script_hash,
        owner_lock,
    } = verify_withdrawal_proof(lock_args)?;
    let raw_withdrawal = withdrawal.raw();

    // check rollup chain id
    let expected_rollup_chain_id: u32 = rollup_config.compatible_chain_id().unpack();
    let chain_id: u64 = raw_withdrawal.chain_id().unpack();
    // first 32 bits are rollup chain id, the last 32 bits are polyjuice chain id
    let rollup_chain_id = (chain_id >> 32) as u32;
    if expected_rollup_chain_id != rollup_chain_id {
        debug!("Withdrawal using wrong rollup_chain_id");
        return Err(Error::WrongSignature);
    }

    // calculate EIP-712 message
    let typed_message = crate::verifications::eip712::types::Withdrawal::from_withdrawal_request(
        withdrawal.raw(),
        owner_lock,
    )?;
    let message = typed_message
        .eip712_message(domain_with_chain_id(raw_withdrawal.chain_id().unpack()).hash_struct());
    // verify sender's script is in the input
    check_l2_account_signature_cell(
        &sender_script_hash,
        gw_types::core::SigningType::Raw,
        message.into(),
    )?;
    Ok(())
}

fn domain_with_chain_id(chain_id: u64) -> EIP712Domain {
    EIP712Domain {
        name: "Godwoken".to_string(),
        chain_id,
        version: "1".to_string(),
        verifying_contract: None,
        salt: None,
    }
}
