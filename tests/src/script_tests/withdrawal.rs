use super::utils::init_env_log;
use super::utils::layer1::build_simple_tx_with_out_point;
use super::utils::rollup::{build_rollup_locked_cell, CellContext};

use crate::testing_tool::programs::{
    ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM, WITHDRAWAL_LOCK_PROGRAM,
};

use ckb_error::assert_error_eq;
use ckb_script::ScriptError;
use ckb_types::prelude::{Builder, Entity, Reader};
use gw_types::bytes::Bytes;
use gw_types::packed::{
    CellDep, CellInput, CellOutput, OutPoint, RollupConfig, Script, UnlockWithdrawalViaTrade,
    UnlockWithdrawalWitness, UnlockWithdrawalWitnessUnion, WithdrawalLockArgs,
    WithdrawalLockArgsReader, WitnessArgs,
};
use gw_types::prelude::{Pack, Unpack};

const INVALID_OUTPUT_ERROR: i8 = 7;
const NOT_FOR_SELL_ERROR: i8 = 19;

#[test]
fn test_unlock_withdrawal_via_trade() {
    init_env_log();

    let rollup_type_hash = random_always_success_script().hash();
    let (mut verify_ctx, script_ctx) = build_verify_context();

    let payee_lock = random_always_success_script();
    let withdrawal_sudt_amount = 500u128;
    let withdrawal_capacity = 1000 * 10u64.pow(8);
    let sell_amount = 450u128;
    let sell_capacity = 800 * 10u64.pow(8);
    let pay_amount = sell_amount;
    let pay_capacity = sell_capacity;
    let payee_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(random_always_success_script().hash().pack())
            .withdrawal_block_hash(random_always_success_script().hash().pack())
            .withdrawal_block_number(rand::random::<u64>().pack())
            .sudt_script_hash(script_ctx.sudt.script.hash().pack())
            .sell_amount(sell_amount.pack())
            .sell_capacity(sell_capacity.pack())
            .owner_lock_hash(payee_lock.hash().pack())
            .payment_lock_hash(payee_lock.hash().pack())
            .build();
        let output = build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            withdrawal_capacity,
            lock_args.as_bytes(),
        );
        (output, withdrawal_sudt_amount.pack().as_bytes())
    };
    let payee_withdrawal_input = {
        let out_point = verify_ctx.insert_cell(
            payee_withdrawal_cell.0.clone(),
            withdrawal_sudt_amount.pack().as_bytes(),
        );
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payee_output_cell = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script.clone()).pack())
            .lock(payee_lock)
            .build();

        (output.to_ckb(), pay_amount.pack().as_bytes())
    };

    let payer_lock = random_always_success_script();
    let payer_input = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script).pack())
            .lock(payer_lock.clone())
            .build();

        let out_point = verify_ctx.insert_cell(output.to_ckb(), pay_amount.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payer_withdrawal_cell = {
        let lock_args = {
            let payee_args = extract_withdrawal_lock(&payee_withdrawal_cell.0.to_gw()).as_builder();
            payee_args
                .owner_lock_hash(payer_lock.hash().pack())
                .payment_lock_hash(payer_lock.hash().pack())
                .build()
        };
        build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            payee_withdrawal_cell.0.to_gw().capacity().unpack(),
            lock_args.as_bytes(),
        )
    };
    let unlock_via_trade_witness = {
        let unlock_args = UnlockWithdrawalViaTrade::new_builder()
            .owner_lock(payer_withdrawal_cell.lock().to_gw())
            .build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaTrade(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    let tx = build_simple_tx_with_out_point(
        &mut verify_ctx.inner,
        payee_withdrawal_cell,
        payee_withdrawal_input.to_ckb().previous_output(),
        payee_output_cell,
    )
    .as_advanced_builder()
    .witness(unlock_via_trade_witness.as_bytes().to_ckb())
    .input(payer_input.to_ckb())
    .witness(Default::default())
    .output(payer_withdrawal_cell)
    .output_data(withdrawal_sudt_amount.pack().as_bytes().to_ckb())
    .cell_dep(script_ctx.withdrawal.dep.to_ckb())
    .cell_dep(script_ctx.sudt.dep.to_ckb())
    .build();

    verify_ctx.verify_tx(tx).expect("success");
}

#[test]
fn test_unlock_withdrawal_not_for_sell_via_trade() {
    init_env_log();

    let rollup_type_hash = random_always_success_script().hash();
    let (mut verify_ctx, script_ctx) = build_verify_context();

    let payee_lock = random_always_success_script();
    let withdrawal_sudt_amount = 500u128;
    let withdrawal_capacity = 1000 * 10u64.pow(8);
    // ERROR: Not for sell
    let sell_amount = 0u128;
    let sell_capacity = 0u64;
    let pay_amount = 500u128;
    let pay_capacity = 1000 * 10u64.pow(8);
    let payee_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(random_always_success_script().hash().pack())
            .withdrawal_block_hash(random_always_success_script().hash().pack())
            .withdrawal_block_number(rand::random::<u64>().pack())
            .sudt_script_hash(script_ctx.sudt.script.hash().pack())
            .sell_amount(sell_amount.pack())
            .sell_capacity(sell_capacity.pack())
            .owner_lock_hash(payee_lock.hash().pack())
            .payment_lock_hash(payee_lock.hash().pack())
            .build();
        let output = build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            withdrawal_capacity,
            lock_args.as_bytes(),
        );
        (output, withdrawal_sudt_amount.pack().as_bytes())
    };
    let payee_withdrawal_input = {
        let out_point = verify_ctx.insert_cell(
            payee_withdrawal_cell.0.clone(),
            withdrawal_sudt_amount.pack().as_bytes(),
        );
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payee_output_cell = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script.clone()).pack())
            .lock(payee_lock)
            .build();

        (output.to_ckb(), pay_amount.pack().as_bytes())
    };

    let payer_lock = random_always_success_script();
    let payer_input = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script).pack())
            .lock(payer_lock.clone())
            .build();

        let out_point = verify_ctx.insert_cell(output.to_ckb(), pay_amount.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payer_withdrawal_cell = {
        let lock_args = {
            let payee_args = extract_withdrawal_lock(&payee_withdrawal_cell.0.to_gw()).as_builder();
            payee_args
                .owner_lock_hash(payer_lock.hash().pack())
                .payment_lock_hash(payer_lock.hash().pack())
                .build()
        };
        build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            payee_withdrawal_cell.0.to_gw().capacity().unpack(),
            lock_args.as_bytes(),
        )
    };
    let unlock_via_trade_witness = {
        let unlock_args = UnlockWithdrawalViaTrade::new_builder()
            .owner_lock(payer_withdrawal_cell.lock().to_gw())
            .build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaTrade(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    let tx = build_simple_tx_with_out_point(
        &mut verify_ctx.inner,
        payee_withdrawal_cell,
        payee_withdrawal_input.to_ckb().previous_output(),
        payee_output_cell,
    )
    .as_advanced_builder()
    .witness(unlock_via_trade_witness.as_bytes().to_ckb())
    .input(payer_input.to_ckb())
    .witness(Default::default())
    .output(payer_withdrawal_cell)
    .output_data(withdrawal_sudt_amount.pack().as_bytes().to_ckb())
    .cell_dep(script_ctx.withdrawal.dep.to_ckb())
    .cell_dep(script_ctx.sudt.dep.to_ckb())
    .build();

    let err = verify_ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-type-hash/{}",
            ckb_types::H256(script_ctx.withdrawal.script.hash())
        ),
        NOT_FOR_SELL_ERROR,
    )
    .input_lock_script(0);
    assert_error_eq!(err, expected_err);
}

#[test]
fn test_unlock_withdrawal_with_modified_withdrawal_lock_args() {
    init_env_log();

    let rollup_type_hash = random_always_success_script().hash();
    let (mut verify_ctx, script_ctx) = build_verify_context();

    let payee_lock = random_always_success_script();
    let withdrawal_sudt_amount = 500u128;
    let withdrawal_capacity = 1000 * 10u64.pow(8);
    let sell_amount = 450u128;
    let sell_capacity = 800 * 10u64.pow(8);
    let pay_amount = sell_amount;
    let pay_capacity = sell_capacity;
    let payee_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(random_always_success_script().hash().pack())
            .withdrawal_block_hash(random_always_success_script().hash().pack())
            .withdrawal_block_number(rand::random::<u64>().pack())
            .sudt_script_hash(script_ctx.sudt.script.hash().pack())
            .sell_amount(sell_amount.pack())
            .sell_capacity(sell_capacity.pack())
            .owner_lock_hash(payee_lock.hash().pack())
            .payment_lock_hash(payee_lock.hash().pack())
            .build();
        let output = build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            withdrawal_capacity,
            lock_args.as_bytes(),
        );
        (output, withdrawal_sudt_amount.pack().as_bytes())
    };
    let payee_withdrawal_input = {
        let out_point = verify_ctx.insert_cell(
            payee_withdrawal_cell.0.clone(),
            withdrawal_sudt_amount.pack().as_bytes(),
        );
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payee_output_cell = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script.clone()).pack())
            .lock(payee_lock)
            .build();

        (output.to_ckb(), pay_amount.pack().as_bytes())
    };

    let payer_lock = random_always_success_script();
    let payer_input = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script).pack())
            .lock(payer_lock.clone())
            .build();

        let out_point = verify_ctx.insert_cell(output.to_ckb(), pay_amount.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    // ERROR: modify withdrawal lock args other fields
    let payer_withdrawal_cell = {
        let lock_args = {
            let payee_args = extract_withdrawal_lock(&payee_withdrawal_cell.0.to_gw()).as_builder();
            payee_args
                .owner_lock_hash(payer_lock.hash().pack())
                .payment_lock_hash(payer_lock.hash().pack())
                .withdrawal_block_number(0u64.pack())
                .build()
        };
        build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            payee_withdrawal_cell.0.to_gw().capacity().unpack(),
            lock_args.as_bytes(),
        )
    };
    let unlock_via_trade_witness = {
        let unlock_args = UnlockWithdrawalViaTrade::new_builder()
            .owner_lock(payer_withdrawal_cell.lock().to_gw())
            .build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaTrade(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    let tx = build_simple_tx_with_out_point(
        &mut verify_ctx.inner,
        payee_withdrawal_cell,
        payee_withdrawal_input.to_ckb().previous_output(),
        payee_output_cell,
    )
    .as_advanced_builder()
    .witness(unlock_via_trade_witness.as_bytes().to_ckb())
    .input(payer_input.to_ckb())
    .witness(Default::default())
    .output(payer_withdrawal_cell)
    .output_data(withdrawal_sudt_amount.pack().as_bytes().to_ckb())
    .cell_dep(script_ctx.withdrawal.dep.to_ckb())
    .cell_dep(script_ctx.sudt.dep.to_ckb())
    .build();

    let err = verify_ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-type-hash/{}",
            ckb_types::H256(script_ctx.withdrawal.script.hash())
        ),
        INVALID_OUTPUT_ERROR,
    )
    .input_lock_script(0);
    assert_error_eq!(err, expected_err);
}

#[test]
fn test_unlock_withdrawal_with_modified_withdrawal_cell() {
    init_env_log();

    let rollup_type_hash = random_always_success_script().hash();
    let (mut verify_ctx, script_ctx) = build_verify_context();

    let payee_lock = random_always_success_script();
    let withdrawal_sudt_amount = 500u128;
    let withdrawal_capacity = 1000 * 10u64.pow(8);
    let sell_amount = 450u128;
    let sell_capacity = 800 * 10u64.pow(8);
    let pay_amount = sell_amount;
    let pay_capacity = sell_capacity;
    let payee_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(random_always_success_script().hash().pack())
            .withdrawal_block_hash(random_always_success_script().hash().pack())
            .withdrawal_block_number(rand::random::<u64>().pack())
            .sudt_script_hash(script_ctx.sudt.script.hash().pack())
            .sell_amount(sell_amount.pack())
            .sell_capacity(sell_capacity.pack())
            .owner_lock_hash(payee_lock.hash().pack())
            .payment_lock_hash(payee_lock.hash().pack())
            .build();
        let output = build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            withdrawal_capacity,
            lock_args.as_bytes(),
        );
        (output, withdrawal_sudt_amount.pack().as_bytes())
    };
    let payee_withdrawal_input = {
        let out_point = verify_ctx.insert_cell(
            payee_withdrawal_cell.0.clone(),
            withdrawal_sudt_amount.pack().as_bytes(),
        );
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payee_output_cell = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script.clone()).pack())
            .lock(payee_lock)
            .build();

        (output.to_ckb(), pay_amount.pack().as_bytes())
    };

    let payer_lock = random_always_success_script();
    let payer_input = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script).pack())
            .lock(payer_lock.clone())
            .build();

        let out_point = verify_ctx.insert_cell(output.to_ckb(), pay_amount.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payer_withdrawal_cell = {
        let lock_args = {
            let payee_args = extract_withdrawal_lock(&payee_withdrawal_cell.0.to_gw()).as_builder();
            payee_args
                .owner_lock_hash(payer_lock.hash().pack())
                .payment_lock_hash(payer_lock.hash().pack())
                .build()
        };
        // ERROR: modify withdrawal cell capacity
        let payee_withdrawal_cell_capacity = payee_withdrawal_cell.0.to_gw().capacity().unpack();
        build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            payee_withdrawal_cell_capacity.saturating_add(1000),
            lock_args.as_bytes(),
        )
    };
    let unlock_via_trade_witness = {
        let unlock_args = UnlockWithdrawalViaTrade::new_builder()
            .owner_lock(payer_withdrawal_cell.lock().to_gw())
            .build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaTrade(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    let tx = build_simple_tx_with_out_point(
        &mut verify_ctx.inner,
        payee_withdrawal_cell,
        payee_withdrawal_input.to_ckb().previous_output(),
        payee_output_cell,
    )
    .as_advanced_builder()
    .witness(unlock_via_trade_witness.as_bytes().to_ckb())
    .input(payer_input.to_ckb())
    .witness(Default::default())
    .output(payer_withdrawal_cell)
    .output_data(withdrawal_sudt_amount.pack().as_bytes().to_ckb())
    .cell_dep(script_ctx.withdrawal.dep.to_ckb())
    .cell_dep(script_ctx.sudt.dep.to_ckb())
    .build();

    let err = verify_ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-type-hash/{}",
            ckb_types::H256(script_ctx.withdrawal.script.hash())
        ),
        INVALID_OUTPUT_ERROR,
    )
    .input_lock_script(0);
    assert_error_eq!(err, expected_err);
}

#[test]
fn test_unlock_withdrawal_with_modified_withdrawal_lock_code_hash() {
    init_env_log();

    let rollup_type_hash = random_always_success_script().hash();
    let (mut verify_ctx, script_ctx) = build_verify_context();

    let payee_lock = random_always_success_script();
    let withdrawal_sudt_amount = 500u128;
    let withdrawal_capacity = 1000 * 10u64.pow(8);
    let sell_amount = 450u128;
    let sell_capacity = 800 * 10u64.pow(8);
    let pay_amount = sell_amount;
    let pay_capacity = sell_capacity;
    let payee_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .account_script_hash(random_always_success_script().hash().pack())
            .withdrawal_block_hash(random_always_success_script().hash().pack())
            .withdrawal_block_number(rand::random::<u64>().pack())
            .sudt_script_hash(script_ctx.sudt.script.hash().pack())
            .sell_amount(sell_amount.pack())
            .sell_capacity(sell_capacity.pack())
            .owner_lock_hash(payee_lock.hash().pack())
            .payment_lock_hash(payee_lock.hash().pack())
            .build();
        let output = build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.withdrawal.script.hash(),
            withdrawal_capacity,
            lock_args.as_bytes(),
        );
        (output, withdrawal_sudt_amount.pack().as_bytes())
    };
    let payee_withdrawal_input = {
        let out_point = verify_ctx.insert_cell(
            payee_withdrawal_cell.0.clone(),
            withdrawal_sudt_amount.pack().as_bytes(),
        );
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payee_output_cell = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script.clone()).pack())
            .lock(payee_lock)
            .build();

        (output.to_ckb(), pay_amount.pack().as_bytes())
    };

    let payer_lock = random_always_success_script();
    let payer_input = {
        let output = CellOutput::new_builder()
            .capacity(pay_capacity.pack())
            .type_(Some(script_ctx.sudt.script.clone()).pack())
            .lock(payer_lock.clone())
            .build();

        let out_point = verify_ctx.insert_cell(output.to_ckb(), pay_amount.pack().as_bytes());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let payer_withdrawal_cell = {
        let lock_args = {
            let payee_args = extract_withdrawal_lock(&payee_withdrawal_cell.0.to_gw()).as_builder();
            payee_args
                .owner_lock_hash(payer_lock.hash().pack())
                .payment_lock_hash(payer_lock.hash().pack())
                .build()
        };
        // ERROR: modify withdrawal cell lock code hash
        build_rollup_locked_cell(
            &rollup_type_hash,
            &script_ctx.sudt.script.hash(),
            payee_withdrawal_cell.0.to_gw().capacity().unpack(),
            lock_args.as_bytes(),
        )
    };
    let unlock_via_trade_witness = {
        let unlock_args = UnlockWithdrawalViaTrade::new_builder()
            .owner_lock(payer_withdrawal_cell.lock().to_gw())
            .build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaTrade(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    let tx = build_simple_tx_with_out_point(
        &mut verify_ctx.inner,
        payee_withdrawal_cell,
        payee_withdrawal_input.to_ckb().previous_output(),
        payee_output_cell,
    )
    .as_advanced_builder()
    .witness(unlock_via_trade_witness.as_bytes().to_ckb())
    .input(payer_input.to_ckb())
    .witness(Default::default())
    .output(payer_withdrawal_cell)
    .output_data(withdrawal_sudt_amount.pack().as_bytes().to_ckb())
    .cell_dep(script_ctx.withdrawal.dep.to_ckb())
    .cell_dep(script_ctx.sudt.dep.to_ckb())
    .build();

    let err = verify_ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-type-hash/{}",
            ckb_types::H256(script_ctx.withdrawal.script.hash())
        ),
        INVALID_OUTPUT_ERROR,
    )
    .input_lock_script(0);
    assert_error_eq!(err, expected_err);
}

struct ScriptDep {
    script: Script,
    dep: CellDep,
}

struct ScriptContext {
    withdrawal: ScriptDep,
    sudt: ScriptDep,
}

fn build_verify_context() -> (CellContext, ScriptContext) {
    let withdrawal_lock_type = random_always_success_script();
    let sudt_type = random_always_success_script();

    let config = RollupConfig::new_builder()
        .withdrawal_script_type_hash(withdrawal_lock_type.hash().pack())
        .l1_sudt_script_type_hash(sudt_type.hash().pack())
        .finality_blocks(10u64.pack())
        .build();
    let mut ctx = CellContext::new(&config, Default::default());

    let withdrawal_output = CellOutput::new_builder()
        .lock(random_always_success_script())
        .type_(Some(withdrawal_lock_type.clone()).pack())
        .build();
    let withdrawal_cell_dep = {
        let out_point =
            ctx.insert_cell(withdrawal_output.to_ckb(), WITHDRAWAL_LOCK_PROGRAM.clone());
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };
    ctx.withdrawal_lock_dep = withdrawal_cell_dep.to_ckb();

    let sudt_output = CellOutput::new_builder()
        .lock(random_always_success_script())
        .type_(Some(sudt_type.clone()).pack())
        .build();
    let sudt_cell_dep = {
        let out_point = ctx.insert_cell(sudt_output.to_ckb(), ALWAYS_SUCCESS_PROGRAM.clone());
        CellDep::new_builder().out_point(out_point.to_gw()).build()
    };

    let script_ctx = ScriptContext {
        withdrawal: ScriptDep {
            script: withdrawal_lock_type,
            dep: withdrawal_cell_dep,
        },
        sudt: ScriptDep {
            script: sudt_type,
            dep: sudt_cell_dep,
        },
    };

    (ctx, script_ctx)
}

fn random_always_success_script() -> Script {
    let random_bytes: [u8; 32] = rand::random();
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .args(Bytes::from(random_bytes.to_vec()).pack())
        .build()
}

fn extract_withdrawal_lock(cell: &CellOutput) -> WithdrawalLockArgs {
    let args: Bytes = cell.lock().args().unpack();
    match WithdrawalLockArgsReader::verify(&args.slice(32..), false) {
        Ok(()) => WithdrawalLockArgs::new_unchecked(args.slice(32..)),
        Err(_) => panic!("invalid withdrawal lock args"),
    }
}

mod conversion {
    use ckb_types::packed::{Bytes, CellDep, CellInput, CellOutput, OutPoint, Script, WitnessArgs};
    use ckb_types::prelude::{Entity, Pack};

    pub trait ToCKBType<T> {
        fn to_ckb(&self) -> T;
    }

    macro_rules! impl_to_ckb {
        ($type_:tt) => {
            impl ToCKBType<$type_> for super::$type_ {
                fn to_ckb(&self) -> $type_ {
                    $type_::new_unchecked(self.as_bytes())
                }
            }
        };
    }
    impl_to_ckb!(Script);
    impl_to_ckb!(CellInput);
    impl_to_ckb!(CellOutput);
    impl_to_ckb!(WitnessArgs);
    impl_to_ckb!(CellDep);

    impl ToCKBType<Bytes> for super::Bytes {
        fn to_ckb(&self) -> Bytes {
            self.pack()
        }
    }

    pub trait ToGWType<T> {
        fn to_gw(&self) -> T;
    }

    macro_rules! impl_to_gw {
        ($type_:tt) => {
            impl ToGWType<super::$type_> for $type_ {
                fn to_gw(&self) -> super::$type_ {
                    super::$type_::new_unchecked(self.as_bytes())
                }
            }
        };
    }

    impl_to_gw!(OutPoint);
    impl_to_gw!(CellOutput);
    impl_to_gw!(Script);
}
use conversion::{ToCKBType, ToGWType};
