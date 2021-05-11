use blake2b::new_blake2b;
use ckb_types::bytes::Bytes;
use gw_common::blake2b;
use lazy_static::lazy_static;
use std::{fs, io::Read, path::PathBuf};

const SCRIPT_DIR: &'static str = "../build/debug";
const CHALLENGE_LOCK_PATH: &'static str = "challenge-lock";
const STATE_VALIDATOR: &'static str = "state-validator";
const ALWAYS_SUCCESS_PATH: &'static str = "always-success";
const ETH_LOCK_PATH: &'static str = "eth-account-lock";
const SECP256K1_DATA_PATH: &'static str = "../c/deps/ckb-production-scripts/build/secp256k1_data";

lazy_static! {
    pub static ref ALWAYS_SUCCESS_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&ALWAYS_SUCCESS_PATH);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref ALWAYS_SUCCESS_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ALWAYS_SUCCESS_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref CHALLENGE_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&CHALLENGE_LOCK_PATH);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref CHALLENGE_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&CHALLENGE_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref STATE_VALIDATOR_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&STATE_VALIDATOR);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref STATE_VALIDATOR_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&STATE_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref ETH_ACCOUNT_LOCK_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&ETH_LOCK_PATH);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref ETH_ACCOUNT_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ETH_ACCOUNT_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref SECP256K1_DATA: Bytes = {
        let mut buf = Vec::new();
        let mut f = fs::File::open(&SECP256K1_DATA_PATH).expect("load secp256k1 data");
        f.read_to_end(&mut buf).expect("read secp256k1 data");
        Bytes::from(buf.to_vec())
    };
    pub static ref SECP256K1_DATA_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&SECP256K1_DATA);
        hasher.finalize(&mut buf);
        buf
    };
}
