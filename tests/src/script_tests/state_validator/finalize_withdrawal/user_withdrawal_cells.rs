use ckb_types::prelude::Entity;
use gw_common::H256;
use gw_types::{
    bytes::Bytes,
    packed::CustodianLockArgs,
    prelude::{Builder, Pack, Unpack},
};

use super::{TestCase, BLOCK_ALL_WITHDRAWALS, CKB};

const ERROR_AMOUNT_OVERFLOW: i8 = 14;
const ERROR_INVALID_USER_WITHDRAWAL_CELL: i8 = 48;
const ERROR_INVALID_CUSTODIAN_CELL: i8 = 28;

fn sample_case() -> TestCase {
    TestCase::builder()
        .push_empty_block(0)
        .push_withdrawal(1, 1999 * CKB, 87)
        .push_withdrawal(2, 391 * CKB, 0)
        .push_withdrawal(2, 301 * CKB, 1)
        .push_empty_block(3)
        .push_empty_block(4)
        .push_empty_block(5)
        .push_withdrawal(6, 401 * CKB, 100)
        .push_withdrawal(6, 666 * CKB, 22)
        .push_withdrawal(6, 777 * CKB, 33)
        .last_finalized_block(6)
        .prev_last_finalized_withdrawal(0, BLOCK_ALL_WITHDRAWALS)
        .post_last_finalized_withdrawal(6, BLOCK_ALL_WITHDRAWALS)
        .build()
}

#[test]
fn test_sample_case() {
    sample_case().verify().expect("pass");
}

#[test]
fn test_user_withdrawal_merge_and_split() {
    let test_case = TestCase::builder()
        .push_empty_block(0)
        .push_withdrawal(1, 1000 * CKB, 87)
        .push_withdrawal(2, 2999 * CKB, 100)
        .last_finalized_block(2)
        .prev_last_finalized_withdrawal(0, BLOCK_ALL_WITHDRAWALS)
        .post_last_finalized_withdrawal(2, BLOCK_ALL_WITHDRAWALS)
        .build();

    test_case.verify().expect("pass");

    // Merge user withdrawal cells from same lock
    {
        let mut case_builder = test_case.clone().into_builder();

        // Use same lock for test withdrawals
        let block_one_withdrawals_mut = case_builder.withdrawals.get_mut(&1).unwrap();
        let user_lock = block_one_withdrawals_mut.first().unwrap().lock.clone();
        let sudt_type = block_one_withdrawals_mut.first().unwrap().type_.clone();

        let block_two_withdrawals_mut = case_builder.withdrawals.get_mut(&2).unwrap();
        block_two_withdrawals_mut.first_mut().unwrap().lock = user_lock.clone();
        block_two_withdrawals_mut.first_mut().unwrap().type_ = sudt_type;

        let mut test_case = case_builder.build();
        assert_eq!(test_case.user_withdrawal_cells.len(), 1);

        let user_lock_hash: H256 = user_lock.hash().into();
        let withdrawal_cells_mut = test_case
            .user_withdrawal_cells
            .get_mut(&user_lock_hash)
            .unwrap();
        assert_eq!(withdrawal_cells_mut.len(), 2);

        // Merge output user withdrawal cells
        let mut merged_withdrawal_cell = withdrawal_cells_mut.get(0).unwrap().clone();
        let (total_capacity, total_sudt_amount) =
            withdrawal_cells_mut
                .iter()
                .fold((0u64, 0u128), |mut acc, cell| {
                    acc.0 = acc.0.saturating_add(cell.capacity);
                    acc.1 = acc.1.saturating_add(cell.sudt_amount);
                    acc
                });

        merged_withdrawal_cell.capacity = total_capacity;
        merged_withdrawal_cell.sudt_amount = total_sudt_amount;

        *withdrawal_cells_mut = vec![merged_withdrawal_cell];
        test_case.verify().expect("pass");
    }

    // Split user withdrawal cell into pure ckb and sudt
    {
        let mut test_case = test_case;
        let lock_hash: H256 = {
            let block_one_withdrawals = test_case.builder.withdrawals.get(&1).unwrap();
            block_one_withdrawals.first().unwrap().lock.hash().into()
        };

        // Split first cell into pure ckb and sudt
        let withdrawal_cells_mut = test_case.user_withdrawal_cells.get_mut(&lock_hash).unwrap();
        assert_eq!(withdrawal_cells_mut.len(), 1);

        let first_cell = withdrawal_cells_mut.first().unwrap();
        let pure_ckb_cell = {
            assert!(first_cell.capacity > 800);
            let mut cell = first_cell.clone();
            cell.capacity = first_cell.capacity.saturating_sub(300);
            cell.sudt_amount = 0;
            cell.type_ = None;
            cell
        };
        let sudt_cell = {
            let mut cell = first_cell.clone();
            cell.capacity = first_cell.capacity.saturating_sub(pure_ckb_cell.capacity);
            cell
        };

        *withdrawal_cells_mut = vec![pure_ckb_cell, sudt_cell];

        let withdrawal_cells = test_case.user_withdrawal_cells.get(&lock_hash).unwrap();
        assert_eq!(withdrawal_cells.len(), 2);

        test_case.verify().expect("pass");
    }
}

#[test]
fn test_input_custodian_balance_not_enough() {
    let test_case = sample_case();

    // Ckb balance not enough
    {
        let mut test_case = test_case.clone();

        let input_mut = test_case.input_custodian_cells.first_mut().unwrap();
        let output = test_case.output_custodian_cells.first().unwrap();

        input_mut.capacity = input_mut
            .capacity
            .checked_sub(output.capacity)
            .unwrap_or(output.capacity + 1);

        expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
    }

    // Sudt balance not enough
    {
        let mut test_case = test_case.clone();

        let input_mut = test_case.input_custodian_cells.first_mut().unwrap();
        let output = test_case.output_custodian_cells.first().unwrap();

        input_mut.sudt_amount = input_mut
            .sudt_amount
            .checked_sub(output.sudt_amount)
            .unwrap_or(output.sudt_amount + 1);

        expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
    }

    // Sudt balance not enough by change sudt type script
    {
        let mut test_case = test_case;

        let input_mut = test_case.input_custodian_cells.first_mut().unwrap();
        input_mut.type_ = input_mut.type_.clone().map(|s| {
            let mut args = Unpack::<Bytes>::unpack(&s.args()).to_vec();

            if let Some(byte) = args.first_mut() {
                *byte = byte.checked_sub(1).unwrap_or(2);
            }

            s.as_builder().args(args.pack()).build()
        });

        expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
    }
}

#[test]
fn test_output_custodian_incorrect_balance() {
    let test_case = sample_case();

    // Ckb balance not enough
    {
        let mut test_case = test_case.clone();

        let output_mut = test_case.output_custodian_cells.first_mut().unwrap();
        output_mut.capacity = output_mut.capacity.checked_sub(1000).unwrap_or(1001);

        expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
    }

    // Sudt balance not enough
    {
        let mut test_case = test_case.clone();

        let output_mut = test_case.output_custodian_cells.first_mut().unwrap();
        output_mut.sudt_amount = output_mut.sudt_amount.checked_sub(1000).unwrap_or(1001);

        expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
    }

    // Sudt balance not enough by change sudt type script
    {
        let mut test_case = test_case;

        let output_mut = test_case.output_custodian_cells.first_mut().unwrap();
        output_mut.type_ = output_mut.type_.clone().map(|s| {
            let mut args = Unpack::<Bytes>::unpack(&s.args()).to_vec();

            if let Some(byte) = args.first_mut() {
                *byte = byte.checked_sub(1).unwrap_or(2);
            }

            s.as_builder().args(args.pack()).build()
        });

        expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
    }
}

#[test]
#[ignore = "collect ckb capacity u64 into u128 balance"]
fn test_build_withdarwal_request_assets_ckb_balance_overflow() {
    unreachable!()
}

#[test]
fn test_build_withdarwal_request_assets_sudt_balance_overflow() {
    let mut builder = sample_case().into_builder();

    let withdrawal_cells_mut = builder.withdrawals.get_mut(&6).unwrap();
    assert!(withdrawal_cells_mut.len() > 1);

    let first_withdrawal_mut = withdrawal_cells_mut.first_mut().unwrap();
    first_withdrawal_mut.sudt_amount = u128::MAX;
    let type_ = first_withdrawal_mut.type_.clone();
    let lock = first_withdrawal_mut.lock.clone();

    let last_withdrawal_mut = withdrawal_cells_mut.last_mut().unwrap();
    last_withdrawal_mut.sudt_amount = u128::MAX;
    last_withdrawal_mut.type_ = type_;
    last_withdrawal_mut.lock = lock;

    let mut test_case = builder.build();

    test_case.input_custodian_cells.iter_mut().for_each(|c| {
        if u128::MAX == c.sudt_amount {
            c.sudt_amount = 1000u128;
        }
    });

    expect_err!(test_case, ERROR_AMOUNT_OVERFLOW);
}

#[test]
fn test_unfullfill_withdrawal_request() {
    let test_case = sample_case();

    // Ckb balance not enough
    {
        let mut test_case = test_case.clone();

        let withdrawal_cells_mut = test_case.user_withdrawal_cells.values_mut().last().unwrap();
        let withdrawal_mut = withdrawal_cells_mut.first_mut().unwrap();
        withdrawal_mut.capacity = withdrawal_mut.capacity.checked_sub(1).unwrap();

        expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
    }

    // Sudt balance not enough
    {
        let mut test_case = test_case;

        let withdrawal_mut = test_case
            .user_withdrawal_cells
            .values_mut()
            .flatten()
            .find(|w| w.type_.is_some())
            .unwrap();

        withdrawal_mut.sudt_amount = withdrawal_mut.sudt_amount.checked_sub(1).unwrap();

        expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
    }
}

#[test]
fn test_sub_balance_from_withdrawal_cell_balance_overflow() {
    let test_case = sample_case();

    // Ckb balance overflow
    {
        let mut test_case = test_case.clone();

        let withdrawal_cells_mut = test_case.user_withdrawal_cells.values_mut().last().unwrap();
        let withdrawal_mut = withdrawal_cells_mut.first_mut().unwrap();
        withdrawal_mut.capacity = u64::MAX;

        expect_err!(test_case, ERROR_AMOUNT_OVERFLOW);
    }

    // Sudt balance not enough
    {
        let mut test_case = test_case;

        let input_custodian_mut = test_case
            .input_custodian_cells
            .iter_mut()
            .find(|c| c.type_.is_some())
            .unwrap();
        input_custodian_mut.sudt_amount = 1000u128;
        let type_ = input_custodian_mut.type_.clone();

        let withdrawal_mut = test_case
            .user_withdrawal_cells
            .values_mut()
            .flatten()
            .find(|w| w.type_ == type_)
            .unwrap();
        withdrawal_mut.sudt_amount = u128::MAX;

        expect_err!(test_case, ERROR_AMOUNT_OVERFLOW);
    }
}

#[test]
fn test_sub_balance_from_withdrawal_cell_unknown_sudt() {
    let mut test_case = sample_case();

    let withdrawal_mut = test_case
        .user_withdrawal_cells
        .values_mut()
        .flatten()
        .find(|w| w.type_.is_some())
        .unwrap();

    withdrawal_mut.type_ = withdrawal_mut.type_.clone().map(|s| {
        let mut args = Unpack::<Bytes>::unpack(&s.args()).to_vec();

        if let Some(byte) = args.first_mut() {
            *byte = byte.checked_sub(1).unwrap_or(2);
        }

        s.as_builder().args(args.pack()).build()
    });

    expect_err!(test_case, ERROR_INVALID_USER_WITHDRAWAL_CELL);
}

#[test]
#[ignore = "collect ckb capacity u64 into u128 balance"]
fn test_collect_finalized_assets_ckb_balance_overflow() {
    unreachable!()
}

#[test]
fn test_collect_finalized_assets_sudt_overflow() {
    let test_case = sample_case();

    // input custodian
    {
        let mut test_case = test_case.clone();

        let input_first_mut = test_case.input_custodian_cells.first_mut().unwrap();
        input_first_mut.sudt_amount = u128::MAX;
        let type_ = input_first_mut.type_.clone();

        let input_last_mut = test_case.input_custodian_cells.last_mut().unwrap();
        input_last_mut.sudt_amount = u128::MAX;
        input_last_mut.type_ = type_;

        expect_err!(test_case, ERROR_AMOUNT_OVERFLOW);
    }

    // output custodian
    {
        let mut test_case = test_case;

        let output_first_mut = test_case.output_custodian_cells.first_mut().unwrap();
        output_first_mut.sudt_amount = u128::MAX;
        let type_ = output_first_mut.type_.clone();

        let output_last_mut = test_case.output_custodian_cells.last_mut().unwrap();
        output_last_mut.sudt_amount = u128::MAX;
        output_last_mut.type_ = type_;

        expect_err!(test_case, ERROR_AMOUNT_OVERFLOW);
    }
}

#[test]
fn test_collect_finalized_assets_has_unfinalized_custodian_cell() {
    let test_case = sample_case();

    let last_finalized_block_number = test_case
        .prev_global_state
        .last_finalized_block_number()
        .unpack();
    let custodian_type_hash: [u8; 32] = test_case
        .rollup_config
        .custodian_script_type_hash()
        .unpack();

    // input custodian
    {
        let mut test_case = test_case.clone();

        let input_first_mut = test_case.input_custodian_cells.first_mut().unwrap();
        input_first_mut.lock_args = CustodianLockArgs::new_builder()
            .deposit_block_number((last_finalized_block_number + 1).pack())
            .build();

        // To unlock unfinalized custodian require reverted deposit cell in output
        let expected_custodian_lock_err = ckb_script::ScriptError::ValidationFailure(
            format!("by-type-hash/{}", ckb_types::H256(custodian_type_hash)),
            1,
        )
        .input_lock_script(1);

        // Either we reach lock error first or state validator also complain about invalid
        // custodian error
        let err_str = test_case.verify().unwrap_err().to_string();
        if err_str != ckb_error::Error::from(expected_custodian_lock_err).to_string() {
            let expected_state_validator_err =
                ckb_error::Error::from(TestCase::expected_err(ERROR_INVALID_CUSTODIAN_CELL))
                    .to_string();
            assert_eq!(err_str, expected_state_validator_err);
        }
    }

    // output custodian
    {
        let mut test_case = test_case;

        let output_first_mut = test_case.output_custodian_cells.first_mut().unwrap();
        output_first_mut.lock_args = CustodianLockArgs::new_builder()
            .deposit_block_number((last_finalized_block_number + 1).pack())
            .build();

        expect_err!(test_case, ERROR_INVALID_CUSTODIAN_CELL);
    }
}
