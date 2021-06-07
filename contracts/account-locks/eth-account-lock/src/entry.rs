// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        debug,
        high_level::load_script,
        syscalls::load_cell_data,
    },
    eth_signature::{extract_eth_lock_args, EthAddress, Secp256k1Eth},
};
use validator_utils::{
    cells::{
        lock_cells::find_challenge_cell,
        rollup::{load_rollup_config, search_rollup_state},
        utils::search_lock_hash,
    },
    ckb_std::high_level::load_witness_args,
    error::Error,
    gw_common::{state::State, H256},
    gw_types::{
        packed::{
            RollupConfig, VerifyTransactionSignatureWitness,
            VerifyTransactionSignatureWitnessReader, VerifyWithdrawalWitness,
            VerifyWithdrawalWitnessReader,
        },
        prelude::*,
    },
    kv_state::KVState,
    signature::SignatureType,
};

/// Eth account lock
/// script args: rollup_script_hash(32 bytes) | eth_address(20 bytes)
/// data: owner_lock_hash(32 bytes) | *message (optional 32 bytes)
pub fn main() -> Result<(), Error> {
    // parse args
    let script = load_script()?;
    let args: Bytes = CKBUnpack::unpack(&script.args());
    let (rollup_script_hash, eth_address) = extract_eth_lock_args(args)?;
    debug!(
        "rollup script hash: {:?} eth_address {:?}",
        &rollup_script_hash, &eth_address
    );

    // parse data
    let (owner_lock_hash, sig_type) = parse_data()?;

    // check owner lock hash cell
    // to prevent others unlock this cell
    if search_lock_hash(&owner_lock_hash, Source::Input).is_none() {
        return Err(Error::OwnerCellNotFound);
    }

    // read rollup config
    let rollup_config = {
        // read global state from rollup cell
        let global_state =
            match search_rollup_state(&(rollup_script_hash.clone()).into(), Source::Input)? {
                Some(state) => state,
                None => return Err(Error::RollupCellNotFound),
            };
        load_rollup_config(&global_state.rollup_config_hash().unpack())?
    };

    // verify signature
    match sig_type {
        SignatureType::Transaction => {
            debug!("Verify tx signature");
            verify_tx_signature(rollup_script_hash, &rollup_config)?;
        }
        SignatureType::Message(msg) => {
            debug!("Verify message signature {:?}", msg);
            verify_message_signature(rollup_script_hash, &rollup_config, eth_address, msg)?;
        }
    }

    Ok(())
}

fn verify_tx_signature(
    rollup_script_hash: H256,
    rollup_config: &RollupConfig,
) -> Result<(), Error> {
    // load tx
    let verify_tx_witness = {
        let witness_lock = load_challenge_witness_args_lock(&rollup_script_hash, rollup_config)?;
        match VerifyTransactionSignatureWitnessReader::verify(&witness_lock, false) {
            Ok(()) => VerifyTransactionSignatureWitness::new_unchecked(witness_lock),
            Err(_err) => {
                debug!("Invalid VerifyTransactionSignatureWitness");
                return Err(Error::InvalidArgs);
            }
        }
    };
    let tx = verify_tx_witness.l2tx();
    let raw_tx = tx.raw();
    let ctx = verify_tx_witness.context();
    let kv_state = KVState::new(
        ctx.kv_state(),
        verify_tx_witness.kv_state_proof().unpack(),
        ctx.account_count().unpack(),
        None,
    );
    let sender_script = {
        let sender_script_hash = kv_state.get_script_hash(raw_tx.from_id().unpack())?;
        ctx.scripts()
            .into_iter()
            .find(|script| sender_script_hash == script.hash().into())
            .ok_or_else(|| {
                debug!("can't find sender script: {:?}", sender_script_hash);
                Error::ScriptNotFound
            })?
    };
    let receiver_script = {
        let receiver_script_hash = kv_state.get_script_hash(raw_tx.to_id().unpack())?;
        ctx.scripts()
            .into_iter()
            .find(|script| receiver_script_hash == script.hash().into())
            .ok_or_else(|| {
                debug!("can't find receiver script: {:?}", receiver_script_hash);
                Error::ScriptNotFound
            })?
    };
    // verify message
    let secp256k1_eth = Secp256k1Eth::default();
    let valid = secp256k1_eth.verify_tx(sender_script, receiver_script, tx)?;
    if !valid {
        debug!("verify tx wrong");
        return Err(Error::WrongSignature);
    }
    Ok(())
}
fn verify_message_signature(
    rollup_script_hash: H256,
    rollup_config: &RollupConfig,
    eth_address: EthAddress,
    message: H256,
) -> Result<(), Error> {
    // load signature
    let signature = {
        let witness_lock = load_challenge_witness_args_lock(&rollup_script_hash, rollup_config)?;
        let verify_withdrawal = match VerifyWithdrawalWitnessReader::verify(&witness_lock, false) {
            Ok(()) => VerifyWithdrawalWitness::new_unchecked(witness_lock),
            Err(_err) => {
                debug!("Invalid VerifyWithdrawalWitness");
                return Err(Error::InvalidArgs);
            }
        };
        verify_withdrawal.withdrawal_request().signature()
    };
    // verify message
    let secp256k1_eth = Secp256k1Eth::default();
    let valid = secp256k1_eth.verify_message(eth_address, signature, message)?;
    if !valid {
        debug!("Wrong signature, message: {:?}", message);
        return Err(Error::WrongSignature);
    }
    Ok(())
}

// parse cell's data
fn parse_data() -> Result<([u8; 32], SignatureType), Error> {
    let mut data = [0u8; 64];
    let loaded_size = load_cell_data(&mut data, 0, 0, Source::GroupInput)?;

    let sig_type = if loaded_size == 32 {
        SignatureType::Transaction
    } else if loaded_size == 64 {
        let mut msg = [0u8; 32];
        msg.copy_from_slice(&data[32..]);
        SignatureType::Message(msg.into())
    } else {
        debug!("Invalid data size: {}", loaded_size);
        return Err(Error::Encoding);
    };

    let mut owner_lock_hash = [0u8; 32];
    owner_lock_hash.copy_from_slice(&data[..32]);

    Ok((owner_lock_hash, sig_type))
}

// locate challenge cell index and load the witness_args.lock
fn load_challenge_witness_args_lock(
    rollup_script_hash: &H256,
    config: &RollupConfig,
) -> Result<Bytes, Error> {
    // find challenge cell
    let challenge_cell = find_challenge_cell(rollup_script_hash, &config, Source::Input)?
        .ok_or_else(|| {
            debug!("not found challenge cell");
            Error::InvalidChallengeCell
        })?;

    // load witness
    let witness_args = load_witness_args(challenge_cell.index, Source::Input)?;
    Ok(witness_args
        .lock()
        .to_opt()
        .map(|lock_bytes| CKBUnpack::unpack(&lock_bytes))
        .unwrap_or_default())
}
