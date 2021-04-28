//! Secp256k1 Eth implementation

use crate::secp256k1_util::recover_uncompressed_key;
use alloc::vec;
use sha3::{Digest, Keccak256};
use validator_utils::{
    ckb_std::debug,
    error::Error,
    gw_common::{blake2b::new_blake2b, H256},
    gw_types::{
        bytes::Bytes,
        packed::{L2Transaction, RawL2Transaction, Script, Signature},
        prelude::*,
    },
};

pub type ETHAddress = [u8; 20];

pub fn extract_eth_lock_args(lock_args: Bytes) -> Result<(H256, ETHAddress), Error> {
    if lock_args.len() != 52 {
        debug!("Invalid lock args len: {}", lock_args.len());
        return Err(Error::InvalidArgs);
    }
    let rollup_script_hash = {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&lock_args[..32]);
        buf.into()
    };
    let eth_address = {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(&lock_args[32..]);
        buf
    };
    Ok((rollup_script_hash, eth_address))
}

#[derive(Default)]
pub struct Secp256k1Eth;

impl Secp256k1Eth {
    fn verify_alone(
        &self,
        eth_address: ETHAddress,
        signature: Signature,
        message: H256,
    ) -> Result<bool, Error> {
        let signature: [u8; 65] = signature.unpack();
        let pubkey = recover_uncompressed_key(message.into(), signature).map_err(|err| {
            debug!("failed to recover secp256k1 pubkey, error number: {}", err);
            Error::WrongSignature
        })?;
        let pubkey_hash = {
            let mut hasher = Keccak256::new();
            hasher.update(&pubkey[1..]);
            let buf = hasher.finalize();
            let mut pubkey_hash = [0u8; 20];
            pubkey_hash.copy_from_slice(&buf[12..]);
            pubkey_hash
        };
        if pubkey_hash != eth_address {
            return Ok(false);
        }
        Ok(true)
    }

    pub fn verify_tx(
        &self,
        rollup_type_hash: H256,
        sender_eth_address: ETHAddress,
        sender_script: Script,
        receiver_script: Script,
        tx: L2Transaction,
    ) -> Result<bool, Error> {
        // verify polyjuice tx
        if let Some(rlp_data) = try_assemble_polyjuice_args(tx.raw(), receiver_script.clone()) {
            let mut hasher = Keccak256::new();
            hasher.update(&rlp_data[..]);
            let buf = hasher.finalize();
            let mut signing_message = [0u8; 32];
            signing_message.copy_from_slice(&buf[..]);
            let signing_message = H256::from(signing_message);
            return self.verify_alone(sender_eth_address, tx.signature(), signing_message);
        }

        // fallback to the tx message
        let message =
            calc_godwoken_signing_message(&rollup_type_hash, &sender_script, &receiver_script, &tx);
        self.verify_message(sender_eth_address, tx.signature(), message)
    }

    // NOTE: verify_tx in this module is using standard Ethereum transaction
    // signing scheme, but verify_withdrawal_signature here is using Ethereum's
    // personal sign(with "\x19Ethereum Signed Message:\n32" appended),
    // this is because verify_tx is designed to provide seamless compatibility
    // with Ethereum, but withdrawal request is a godwoken thing, which
    // do not exist in Ethereum. Personal sign is thus used here.
    pub fn verify_message(
        &self,
        eth_address: ETHAddress,
        signature: Signature,
        message: H256,
    ) -> Result<bool, Error> {
        let mut hasher = Keccak256::new();
        hasher.update("\x19Ethereum Signed Message:\n32");
        hasher.update(message.as_slice());
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        let signing_message = H256::from(signing_message);

        self.verify_alone(eth_address, signature, signing_message)
    }
}

fn try_assemble_polyjuice_args(raw_tx: RawL2Transaction, receiver_script: Script) -> Option<Bytes> {
    let args: Bytes = raw_tx.args().unpack();
    if args.len() < 52 {
        return None;
    }
    if args[0..7] != b"\xFF\xFF\xFFPOLY"[..] {
        return None;
    }
    let mut stream = rlp::RlpStream::new();
    stream.begin_unbounded_list();
    let nonce: u32 = raw_tx.nonce().unpack();
    stream.append(&nonce);
    let gas_price = {
        let mut data = [0u8; 16];
        data.copy_from_slice(&args[16..32]);
        u128::from_le_bytes(data)
    };
    stream.append(&gas_price);
    let gas_limit = {
        let mut data = [0u8; 8];
        data.copy_from_slice(&args[8..16]);
        u64::from_le_bytes(data)
    };
    stream.append(&gas_limit);
    let (to, polyjuice_chain_id) = if args[7] == 3 {
        // 3 for EVMC_CREATE
        // In case of deploying a polyjuice contract, to id(creator account id)
        // is directly used as chain id
        (vec![0u8; 20], raw_tx.to_id().unpack())
    } else {
        // For contract calling, chain id is read from scrpit args of
        // receiver_script, see the following link for more details:
        // https://github.com/nervosnetwork/godwoken-polyjuice#normal-contract-account-script
        if receiver_script.args().len() < 36 {
            return None;
        }
        let polyjuice_chain_id = {
            let mut data = [0u8; 4];
            data.copy_from_slice(&receiver_script.args().raw_data()[32..36]);
            u32::from_le_bytes(data)
        };
        let mut to = vec![0u8; 20];
        let receiver_hash = receiver_script.hash();
        to[0..16].copy_from_slice(&receiver_hash[0..16]);
        let to_id: u32 = raw_tx.to_id().unpack();
        to[16..20].copy_from_slice(&to_id.to_le_bytes());
        (to, polyjuice_chain_id)
    };
    stream.append(&to);
    let value = {
        let mut data = [0u8; 16];
        data.copy_from_slice(&args[32..48]);
        u128::from_le_bytes(data)
    };
    stream.append(&value);
    let payload_length = {
        let mut data = [0u8; 4];
        data.copy_from_slice(&args[48..52]);
        u32::from_le_bytes(data)
    } as usize;
    if args.len() != 52 + payload_length {
        return None;
    }
    stream.append(&args[52..52 + payload_length].to_vec());
    // TODO: read rollup chain id from config cell
    let rollup_chain_id = 0u32;
    let chain_id: u64 = ((rollup_chain_id as u64) << 32) | (polyjuice_chain_id as u64);
    stream.append(&chain_id);
    stream.append(&0u8);
    stream.append(&0u8);
    stream.finalize_unbounded_list();
    Some(Bytes::from(stream.out().to_vec()))
}

fn calc_godwoken_signing_message(
    rollup_type_hash: &H256,
    sender_script: &Script,
    receiver_script: &Script,
    tx: &L2Transaction,
) -> H256 {
    let mut hasher = new_blake2b();
    hasher.update(rollup_type_hash.as_slice());
    hasher.update(&sender_script.hash());
    hasher.update(&receiver_script.hash());
    hasher.update(tx.as_slice());
    let mut message = [0u8; 32];
    hasher.finalize(&mut message);
    message.into()
}
