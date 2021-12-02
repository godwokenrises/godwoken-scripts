// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

use gw_utils::{
    cells::rollup::MAX_ROLLUP_WITNESS_SIZE,
    gw_types::{
        self,
        core::ScriptHashType,
        packed::{
            CustodianLockArgs, CustodianLockArgsReader, RollupActionUnionReader,
            UnlockWithdrawalWitnessUnion, WithdrawalLockArgs, WithdrawalLockArgsReader,
        },
    },
};
use gw_utils::{
    cells::{
        rollup::{
            load_rollup_config, parse_rollup_action, search_rollup_cell, search_rollup_state,
        },
        token::fetch_token_amount_by_lock_hash,
        token::TokenType,
        utils::search_lock_hash,
    },
    ckb_std::high_level::load_cell_lock,
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source,
    ckb_types::{self, bytes::Bytes, prelude::Unpack as CKBUnpack},
    high_level::{
        load_cell_capacity, load_cell_data, load_cell_type_hash, load_script, load_witness_args,
    },
};

use crate::error::Error;
use gw_types::{
    packed::{UnlockWithdrawalWitness, UnlockWithdrawalWitnessReader},
    prelude::*,
};

const FINALIZED_BLOCK_NUMBER: u64 = 0;
const FINALIZED_BLOCK_HASH: [u8; 32] = [0u8; 32];

/// args: rollup_type_hash | withdrawal lock args
fn parse_lock_args(
    script: &ckb_types::packed::Script,
) -> Result<([u8; 32], WithdrawalLockArgs), Error> {
    let mut rollup_type_hash = [0u8; 32];
    let args: Bytes = script.args().unpack();
    if args.len() < rollup_type_hash.len() {
        return Err(Error::InvalidArgs);
    }
    rollup_type_hash.copy_from_slice(&args[..32]);
    match WithdrawalLockArgsReader::verify(&args.slice(32..), false) {
        Ok(()) => Ok((
            rollup_type_hash,
            WithdrawalLockArgs::new_unchecked(args.slice(32..)),
        )),
        Err(_) => Err(Error::InvalidArgs),
    }
}

pub fn main() -> Result<(), Error> {
    let script = load_script()?;
    let (rollup_type_hash, lock_args) = parse_lock_args(&script)?;

    // load unlock arguments from witness
    let witness_args = load_witness_args(0, Source::GroupInput)?;
    let unlock_args = {
        let unlock_args: Bytes = witness_args
            .lock()
            .to_opt()
            .ok_or(Error::InvalidArgs)?
            .unpack();
        match UnlockWithdrawalWitnessReader::verify(&unlock_args, false) {
            Ok(()) => UnlockWithdrawalWitness::new_unchecked(unlock_args),
            Err(_) => return Err(Error::ProofNotFound),
        }
    };

    // execute verification
    match unlock_args.to_enum() {
        UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaRevert(unlock_args) => {
            let mut rollup_action_witness = [0u8; MAX_ROLLUP_WITNESS_SIZE];
            let withdrawal_block_hash = lock_args.withdrawal_block_hash();
            // prove the block is reverted
            let rollup_action = {
                let index = search_rollup_cell(&rollup_type_hash, Source::Output)
                    .ok_or(Error::RollupCellNotFound)?;
                parse_rollup_action(&mut rollup_action_witness, index, Source::Output)?
            };
            match rollup_action.to_enum() {
                RollupActionUnionReader::RollupSubmitBlock(args) => {
                    if !args
                        .reverted_block_hashes()
                        .iter()
                        .any(|hash| hash.as_slice() == withdrawal_block_hash.as_slice())
                    {
                        return Err(Error::InvalidRevertedBlocks);
                    }
                }
                _ => {
                    return Err(Error::InvalidRevertedBlocks);
                }
            }
            let custodian_lock_hash: [u8; 32] = unlock_args.custodian_lock_hash().unpack();
            // check there are a reverted custodian lock in the output
            let custodian_cell_index = match search_lock_hash(&custodian_lock_hash, Source::Output)
            {
                Some(index) => index,
                None => return Err(Error::InvalidOutput),
            };

            // check reverted custodian deposit info.
            let custodian_lock = load_cell_lock(custodian_cell_index, Source::Output)?;
            let custodian_lock_args = {
                let args: Bytes = custodian_lock.args().unpack();
                if args.len() < rollup_type_hash.len() {
                    return Err(Error::InvalidArgs);
                }
                if args[..32] != rollup_type_hash {
                    return Err(Error::InvalidArgs);
                }

                match CustodianLockArgsReader::verify(&args.slice(32..), false) {
                    Ok(_) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                    Err(_) => return Err(Error::InvalidOutput),
                }
            };
            let custodian_deposit_block_hash: [u8; 32] =
                custodian_lock_args.deposit_block_hash().unpack();
            let custodian_deposit_block_number: u64 =
                custodian_lock_args.deposit_block_number().unpack();
            let global_state = search_rollup_state(&rollup_type_hash, Source::Input)?
                .ok_or(Error::RollupCellNotFound)?;
            let config = load_rollup_config(&global_state.rollup_config_hash().unpack())?;
            if custodian_lock.code_hash().as_slice()
                != config.custodian_script_type_hash().as_slice()
                || custodian_lock.hash_type() != ScriptHashType::Type.into()
                || custodian_deposit_block_hash != FINALIZED_BLOCK_HASH
                || custodian_deposit_block_number != FINALIZED_BLOCK_NUMBER
            {
                return Err(Error::InvalidOutput);
            }

            // check capacity, data_hash, type_hash
            check_output_cell_has_same_content(custodian_cell_index)?;
            Ok(())
        }
        UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaFinalize(_unlock_args) => {
            // try search rollup state from deps
            let global_state = match search_rollup_state(&rollup_type_hash, Source::CellDep)? {
                Some(state) => state,
                None => {
                    // then try search rollup state from inputs
                    search_rollup_state(&rollup_type_hash, Source::Input)?
                        .ok_or(Error::RollupCellNotFound)?
                }
            };
            // check finality
            let withdrawal_block_number: u64 = lock_args.withdrawal_block_number().unpack();
            let last_finalized_block_number: u64 =
                global_state.last_finalized_block_number().unpack();

            if withdrawal_block_number > last_finalized_block_number {
                // not yet finalized
                return Err(Error::InvalidArgs);
            }

            // withdrawal lock is finalized, unlock for owner
            if search_lock_hash(&lock_args.owner_lock_hash().unpack(), Source::Input).is_none() {
                return Err(Error::OwnerCellNotFound);
            }
            Ok(())
        }

        UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaTrade(unlock_args) => {
            // rollup cell does not in this tx, which means this is a buying tx
            // return success if tx has enough output send to owner
            // make sure output >= input + sell_amount
            let payment_lock_hash = lock_args.payment_lock_hash().unpack();
            let sudt_script_hash: [u8; 32] = lock_args.sudt_script_hash().unpack();
            let token_type: TokenType = sudt_script_hash.into();
            let input_token =
                fetch_token_amount_by_lock_hash(&payment_lock_hash, &token_type, Source::Input)?;
            let output_token =
                fetch_token_amount_by_lock_hash(&payment_lock_hash, &token_type, Source::Output)?;
            let sell_amount: u128 = lock_args.sell_amount().unpack();
            let sell_capacity: u64 = lock_args.sell_capacity().unpack();
            // Withdrawal cell is not for sell
            if sell_amount == 0 && sell_capacity == 0 {
                return Err(Error::NotForSell);
            }
            let expected_output_amount = input_token
                .total_token_amount
                .checked_add(sell_amount)
                .ok_or(Error::AmountOverflow)?;
            let expected_output_capacity = input_token
                .total_capacity
                .checked_add(sell_capacity as u128)
                .ok_or(Error::AmountOverflow)?;
            if output_token.total_token_amount < expected_output_amount
                || output_token.total_capacity < expected_output_capacity
            {
                return Err(Error::InsufficientAmount);
            }

            let new_lock_hash = unlock_args.owner_lock().hash();
            let index = match search_lock_hash(&new_lock_hash, Source::Output) {
                Some(i) => i,
                None => return Err(Error::InvalidOutput),
            };

            // check new withdraw cell
            check_output_cell_has_same_content(index)?;

            // check new withdrawal lock
            let output_lock = load_cell_lock(index, Source::Output)?;
            if output_lock.code_hash().as_slice() != script.code_hash().as_slice()
                || output_lock.hash_type() != script.hash_type()
            {
                return Err(Error::InvalidOutput);
            }

            // make sure the output should only change owner_lock_hash and payment_lock_hash fields
            let (output_rollup_type_hash, output_lock_args) = parse_lock_args(&output_lock)?;
            let expected_output_lock_args = lock_args
                .as_builder()
                .owner_lock_hash(output_lock_args.owner_lock_hash())
                .payment_lock_hash(output_lock_args.payment_lock_hash())
                .sudt_script_hash(output_lock_args.sudt_script_hash())
                .sell_amount(output_lock_args.sell_amount())
                .sell_capacity(output_lock_args.sell_capacity())
                .build();
            if output_rollup_type_hash != rollup_type_hash
                || output_lock_args.as_slice() != expected_output_lock_args.as_slice()
            {
                return Err(Error::InvalidOutput);
            }

            Ok(())
        }
    }
}

fn check_output_cell_has_same_content(output_index: usize) -> Result<(), Error> {
    if load_cell_capacity(0, Source::GroupInput)?
        != load_cell_capacity(output_index, Source::Output)?
    {
        return Err(Error::InvalidOutput);
    }

    // TODO: use load_cell_data_hash
    // NOTE: load_cell_data_hash from inputs throw ItemMissing error. Comparing data directly
    // as temporary workaround. Right now data should be sudt amount only, 16 bytes long.
    if load_cell_data(0, Source::GroupInput)? != load_cell_data(output_index, Source::Output)? {
        return Err(Error::InvalidOutput);
    }

    if load_cell_type_hash(0, Source::GroupInput)?
        != load_cell_type_hash(output_index, Source::Output)?
    {
        return Err(Error::InvalidOutput);
    }
    Ok(())
}
