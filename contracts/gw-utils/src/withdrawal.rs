use gw_types::{
    bytes::Bytes,
    packed::{Script, ScriptReader, WithdrawalLockArgs, WithdrawalLockArgsReader},
    prelude::{Entity, Reader, Unpack},
};

use crate::error::Error;

pub enum OwnerLock {
    None,
    Owner(Script),
    V1Deposit(Script),
}

pub struct ParsedWithdrawalLockArgs {
    pub lock_args: WithdrawalLockArgs,
    pub owner_lock: OwnerLock,
}

/// args: rollup_type_hash | withdrawal lock args | owner lock len (optional) | owner lock (optional) | withdrawal_to_v1 flag byte (optional)
pub fn parse_lock_args(args: &Bytes) -> Result<ParsedWithdrawalLockArgs, Error> {
    let lock_args_start = 32;
    let lock_args_end = lock_args_start + WithdrawalLockArgs::TOTAL_SIZE;

    let args_len = args.len();
    if args_len < lock_args_end {
        return Err(Error::InvalidArgs);
    }

    let raw_args = args.slice(lock_args_start..lock_args_end);
    let lock_args = match WithdrawalLockArgsReader::verify(&raw_args, false) {
        Ok(()) => WithdrawalLockArgs::new_unchecked(raw_args),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let owner_lock_start = lock_args_end + 4; // u32 length
    if args_len <= owner_lock_start {
        let parsed_args = ParsedWithdrawalLockArgs {
            lock_args,
            owner_lock: OwnerLock::None,
        };
        return Ok(parsed_args);
    }

    let mut owner_lock_len_buf = [0u8; 4];
    owner_lock_len_buf.copy_from_slice(&args.slice(lock_args_end..owner_lock_start));

    let owner_lock_len = u32::from_be_bytes(owner_lock_len_buf) as usize;
    let owner_lock_end = owner_lock_start + owner_lock_len;
    if owner_lock_end != args_len && owner_lock_end + 1 != args_len {
        return Err(Error::InvalidArgs);
    }

    let raw_script = args.slice(owner_lock_start..owner_lock_end);
    let owner_lock = match ScriptReader::verify(&raw_script, false) {
        Ok(()) => Script::new_unchecked(raw_script),
        Err(_) => return Err(Error::InvalidArgs),
    };

    let owner_lock_hash: [u8; 32] = lock_args.owner_lock_hash().unpack();
    if owner_lock.hash() != owner_lock_hash {
        return Err(Error::InvalidArgs);
    }

    let owner_lock = if owner_lock_end + 1 == args_len && args[owner_lock_end] == 1 {
        OwnerLock::V1Deposit(owner_lock)
    } else {
        OwnerLock::Owner(owner_lock)
    };

    let parsed_args = ParsedWithdrawalLockArgs {
        lock_args,
        owner_lock,
    };
    Ok(parsed_args)
}
