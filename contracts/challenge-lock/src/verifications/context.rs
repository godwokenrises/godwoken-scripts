use alloc::vec;
use core::result::Result;
use gw_common::{
    h256_ext::H256Ext,
    merkle_utils::calculate_state_checkpoint,
    smt::{Blake2bHasher, CompiledMerkleProof},
    state::State,
    H256,
};
use gw_types::{
    core::ScriptHashType,
    packed::{ChallengeTarget, L2Transaction, RawL2Block, RollupConfig, ScriptVec},
    prelude::*,
};
use validator_utils::gw_common;
use validator_utils::gw_types;
use validator_utils::{ckb_std::debug, error::Error, kv_state::KVState};

pub struct TxContextInput<'a> {
    pub tx: L2Transaction,
    pub kv_state: KVState,
    pub scripts: ScriptVec,
    pub raw_block: RawL2Block,
    pub rollup_config: &'a RollupConfig,
    pub target: ChallengeTarget,
    pub tx_proof: gw_types::packed::Bytes,
}

pub struct TxContext {
    pub sender_script_hash: H256,
    pub receiver_script_hash: H256,
}

pub fn verify_tx_context(input: TxContextInput) -> Result<TxContext, Error> {
    let TxContextInput {
        tx,
        kv_state,
        scripts,
        raw_block,
        rollup_config,
        target,
        tx_proof,
    } = input;

    let raw_tx = tx.raw();

    // verify tx account's script
    let sender_id: u32 = raw_tx.from_id().unpack();
    let receiver_id: u32 = raw_tx.to_id().unpack();
    let sender_script_hash = kv_state.get_script_hash(sender_id).map_err(|_| {
        debug!("get sender script_hash");
        Error::SMTKeyMissing
    })?;
    let receiver_script_hash = kv_state.get_script_hash(receiver_id).map_err(|_| {
        debug!("get receiver script_hash");
        Error::SMTKeyMissing
    })?;

    // check tx.nonce
    let nonce: u32 = raw_tx.nonce().unpack();
    let sender_nonce = kv_state.get_nonce(sender_id)?;
    if nonce != sender_nonce {
        debug!(
            "invalid nonce, tx.nonce {} sender.nonce {}",
            nonce, sender_nonce
        );
        return Err(Error::UnexpectedTxNonce);
    }

    // find scripts
    let sender_script = scripts
        .clone()
        .into_iter()
        .find(|script| H256::from(script.hash()) == sender_script_hash)
        .ok_or(Error::ScriptNotFound)?;
    let receiver_script = scripts
        .into_iter()
        .find(|script| H256::from(script.hash()) == receiver_script_hash)
        .ok_or(Error::ScriptNotFound)?;

    // sender must be a valid External Owned Account
    if sender_script.hash_type() != ScriptHashType::Type.into() {
        debug!("sender script has invalid script hash type: Data");
        return Err(Error::UnknownEOAScript);
    }
    if rollup_config
        .allowed_eoa_type_hashes()
        .into_iter()
        .find(|code_hash| code_hash == &sender_script.code_hash())
        .is_none()
    {
        debug!(
            "sender script has unknown code_hash: {}",
            sender_script.code_hash()
        );
        return Err(Error::UnknownEOAScript);
    }

    // receiver must be a valid contract account
    if receiver_script.hash_type() != ScriptHashType::Type.into() {
        debug!("receiver script has invalid script hash type: Data");
        return Err(Error::UnknownContractScript);
    }
    if rollup_config
        .allowed_contract_type_hashes()
        .into_iter()
        .find(|code_hash| code_hash == &receiver_script.code_hash())
        .is_none()
    {
        debug!(
            "receiver script has unknown code_hash: {}",
            receiver_script.code_hash()
        );
        return Err(Error::UnknownContractScript);
    }

    // verify block hash
    if raw_block.hash() != target.block_hash().as_slice() {
        debug!(
            "wrong block hash, block_hash: {:?}, target block_hash: {:?}",
            raw_block.hash(),
            target.block_hash()
        );
        return Err(Error::InvalidBlock);
    }

    // verify tx merkle proof
    let tx_witness_root: H256 = raw_block.submit_transactions().tx_witness_root().unpack();
    let tx_index: u32 = target.target_index().unpack();
    let tx_witness_hash: H256 = tx.witness_hash().into();
    let valid = CompiledMerkleProof(tx_proof.unpack())
        .verify::<Blake2bHasher>(
            &tx_witness_root,
            vec![(H256::from_u32(tx_index), tx_witness_hash)],
        )
        .map_err(|_err| {
            debug!("verify_tx_context, merkle proof error: {}", _err);
            Error::MerkleProof
        })?;
    if !valid {
        debug!("wrong tx merkle proof");
        return Err(Error::MerkleProof);
    }

    // verify kv-state merkle proof (prev state root)
    let prev_state_checkpoint: H256 = match tx_index.checked_sub(1) {
        Some(tx_prev_state_checkpoint_index) => {
            // skip withdrawal state checkpoints
            let offset: u32 = raw_block.submit_withdrawals().withdrawal_count().unpack();
            raw_block
                .state_checkpoint_list()
                .get((offset + tx_prev_state_checkpoint_index) as usize)
                .ok_or(Error::InvalidStateCheckpoint)?
                .unpack()
        }
        None => raw_block
            .submit_transactions()
            .prev_state_checkpoint()
            .unpack(),
    };
    let state_root = kv_state.calculate_root().map_err(|_err| {
        debug!("verify_tx_context, calculate merkle root error: {:?}", _err);
        Error::MerkleProof
    })?;
    let account_count = kv_state.get_account_count()?;
    let calculated_state_checkpoint: H256 = calculate_state_checkpoint(&state_root, account_count);
    if prev_state_checkpoint != calculated_state_checkpoint {
        debug!(
            "TxContext mismatch prev_state_checkpoint: {:?}, calculated_state_checkpoint: {:?}",
            prev_state_checkpoint, calculated_state_checkpoint
        );
        return Err(Error::MerkleProof);
    }

    let tx_ctx = TxContext {
        sender_script_hash,
        receiver_script_hash,
    };
    Ok(tx_ctx)
}
