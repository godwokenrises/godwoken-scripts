#![allow(dead_code)]

/* Data Fatals */
pub const GW_FATAL_BUFFER_OVERFLOW: u8 = 100;
pub const GW_FATAL_INVALID_CONTEXT: u8 = 101;
pub const GW_FATAL_INVALID_DATA: u8 = 102;
pub const GW_FATAL_MISMATCH_RETURN_DATA: u8 = 103;
pub const GW_FATAL_UNKNOWN_ARGS: u8 = 104;
pub const GW_FATAL_INVALID_SUDT_SCRIPT: u8 = 105;

/* Notfound Fatals */
pub const GW_FATAL_DATA_CELL_NOT_FOUND: u8 = 110;
pub const GW_FATAL_STATE_KEY_NOT_FOUND: u8 = 111;
pub const GW_FATAL_SIGNATURE_CELL_NOT_FOUND: u8 = 112;
pub const GW_FATAL_ACCOUNT_NOT_FOUND: u8 = 113;

/* Merkle Fatals */
pub const GW_FATAL_INVALID_PROOF: u8 = 120;
pub const GW_FATAL_INVALID_STACK: u8 = 121;
pub const GW_FATAL_INVALID_SIBLING: u8 = 122;

/* User Errors */
pub const GW_ERROR_DUPLICATED_SCRIPT_HASH: u8 = 200;
pub const GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH: u8 = 201;
pub const GW_ERROR_INVALID_CONTRACT_SCRIPT: u8 = 202;
pub const GW_ERROR_NOT_FOUND: u8 = 203;
pub const GW_ERROR_RECOVER: u8 = 204;
