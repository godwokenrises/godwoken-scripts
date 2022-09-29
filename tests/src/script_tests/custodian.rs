#![allow(clippy::mutable_key_type)]

use crate::script_tests::utils::conversion::{CKBTypeIntoExt, ToCKBType, ToGWType};
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::{
    build_simple_tx_with_out_point_and_since, random_always_success_script, random_out_point,
    since_timestamp, state_validator_script_error,
};
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, build_type_id_script,
    calculate_state_validator_type_id, CellContext, CellContextParam,
};
use crate::testing_tool::chain::construct_block;
use crate::testing_tool::chain::setup_chain;
use crate::testing_tool::programs::{
    ALWAYS_SUCCESS_CODE_HASH, CUSTODIAN_LOCK_PROGRAM, STATE_VALIDATOR_CODE_HASH,
};
use gw_types::core::AllowedEoaType;
use gw_types::packed::{AllowedTypeHash, CellOutput, WitnessArgs};
use gw_types::prelude::{Builder, Entity, Pack, PackVec, Unpack};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        CustodianLockArgs, RollupAction, RollupActionUnion, RollupConfig, RollupSubmitBlock,
        Script, StakeLockArgs,
    },
};

const ERROR_INVALID_CUSTODIAN_CELL: i8 = 28;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_rollup_action_submit_block() {
    init_env_log();

    let capacity = 1000_00000000u64;
    let rollup_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(rollup_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash((*STATE_VALIDATOR_CODE_HASH).pack())
            .hash_type(ScriptHashType::Data.into())
            .args(Bytes::from(type_id.to_vec()).pack())
            .build()
    };

    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.to_gw().hash();
    let custodian_lock_type = build_type_id_script(b"custodian_lock_type_id");
    let custodian_script_type_hash: [u8; 32] = custodian_lock_type.to_gw().hash();
    let withdrawal_lock_type = build_type_id_script(b"withdrawal_lock_type_id");
    let withdrawal_script_type_hash: [u8; 32] = withdrawal_lock_type.to_gw().hash();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(stake_script_type_hash.pack())
        .custodian_script_type_hash(custodian_script_type_hash.pack())
        .withdrawal_script_type_hash(withdrawal_script_type_hash.pack())
        .allowed_eoa_type_hashes(
            vec![AllowedTypeHash::new(
                AllowedEoaType::Eth,
                *ALWAYS_SUCCESS_CODE_HASH,
            )]
            .pack(),
        )
        .build();

    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;

    // create a rollup cell
    let rollup_cell = build_always_success_cell(capacity, Some(rollup_type_script.to_ckb()));

    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        custodian_lock_type,
        withdrawal_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);

    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.reset_mem_block(&Default::default()).await.unwrap();
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };

    // build stake input and output
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.stake_script_type_hash().unpack(),
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        ctx.insert_cell(cell, Bytes::default()).into_ext()
    };
    let output_stake_cell = {
        let block_number = block_result.block.raw().number();
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(block_number)
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.stake_script_type_hash().unpack(),
            stake_capacity,
            lock_args.as_bytes(),
        )
    };

    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state
        .clone()
        .as_builder()
        .version(2u8.into())
        .build()
        .as_bytes();

    // build custodian input
    let input_custodian_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.custodian_script_type_hash().unpack(),
            1000 * 10u64.pow(8),
            CustodianLockArgs::default().as_bytes(),
        );

        ctx.insert_cell(cell, Bytes::default()).into_ext()
    };

    // must have same value save input custodian
    let output_custodian_cell = {
        let args = CustodianLockArgs::new_builder()
            .deposit_block_hash([0u8; 32].pack())
            .deposit_block_number(0.pack())
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.custodian_script_type_hash().unpack(),
            1000 * 10u64.pow(8),
            args.as_bytes(),
        )
    };

    // verify submit block
    let tip_block_timestamp = block_result.block.raw().timestamp();
    let rollup_cell_data = block_result
        .global_state
        .as_builder()
        .tip_block_timestamp(tip_block_timestamp.clone())
        .version(2u8.into())
        .build()
        .as_bytes();
    let witness = {
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupSubmitBlock(
                RollupSubmitBlock::new_builder()
                    .block(block_result.block)
                    .build(),
            ))
            .build();
        WitnessArgs::new_builder()
            .output_type(Some(rollup_action.as_bytes()).pack())
            .build()
    };
    let tx = build_simple_tx_with_out_point_and_since(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        (
            rollup_out_point,
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(Bytes::default().to_ckb())
    .input(input_custodian_cell)
    .output(output_custodian_cell)
    .output_data(Bytes::default().to_ckb())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(witness.as_bytes().to_ckb())
    .build();

    ctx.verify_tx(tx).expect("pass");
}

fn append_custodian_cells(
    ctx: &mut CellContext,
    tx: ckb_types::core::TransactionView,
    rollup_type_script: Script,
    rollup_config: RollupConfig,
) -> (ckb_types::core::TransactionView, RollupConfig) {
    let custodian_lock_type = build_type_id_script(b"custodian_lock_type_id");
    let custodian_script_type_hash: [u8; 32] = custodian_lock_type.to_gw().hash();

    let rollup_config = rollup_config
        .as_builder()
        .custodian_script_type_hash(custodian_script_type_hash.pack())
        .build();
    let rollup_config_dep: ckb_types::packed::CellDep = {
        let output = CellOutput::new_builder()
            .capacity((rollup_config.as_bytes().len() as u64).pack())
            .build();
        ctx.insert_cell(output.to_ckb(), rollup_config.as_bytes())
            .into_ext()
    };
    ctx.rollup_config_dep = rollup_config_dep;

    let custodian_lock_dep: ckb_types::packed::CellDep = {
        let output = CellOutput::new_builder()
            .capacity((10000 * 10u64.pow(8)).pack())
            .type_(Some(custodian_lock_type.to_gw()).pack())
            .lock(random_always_success_script().to_gw())
            .build();
        ctx.insert_cell(output.to_ckb(), CUSTODIAN_LOCK_PROGRAM.clone())
            .into_ext()
    };
    ctx.custodian_lock_dep = custodian_lock_dep;

    // build custodian input
    let input_custodian_cell: ckb_types::packed::CellInput = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.custodian_script_type_hash().unpack(),
            1000 * 10u64.pow(8),
            CustodianLockArgs::default().as_bytes(),
        );

        ctx.insert_cell(cell, Bytes::default()).into_ext()
    };

    // must have same value save input custodian
    let output_custodian_cell = {
        let args = CustodianLockArgs::new_builder()
            .deposit_block_hash([0u8; 32].pack())
            .deposit_block_number(0.pack())
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.custodian_script_type_hash().unpack(),
            1000 * 10u64.pow(8),
            args.as_bytes(),
        )
    };

    let tx = tx
        .as_advanced_builder()
        .input(input_custodian_cell)
        .output(output_custodian_cell)
        .output_data(Bytes::default().to_ckb())
        .cell_dep(ctx.custodian_lock_dep.clone())
        .build();

    (tx, rollup_config)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_rollup_action_enter_challenge() {
    use super::state_validator::enter_challenge;

    init_env_log();

    let (mut ctx, tx, rollup_type_script, rollup_config) =
        enter_challenge::sample_test_case().await;

    let (tx, rollup_config) =
        append_custodian_cells(&mut ctx, tx, rollup_type_script, rollup_config);

    let expected_validator_err = state_validator_script_error(ERROR_INVALID_CUSTODIAN_CELL);
    let input_lock_idx = tx.inputs().len() - 1;
    let err_msg = ctx.verify_tx(tx).unwrap_err().to_string();
    if err_msg != ckb_error::Error::from(expected_validator_err).to_string() {
        let expected_lock_err = ckb_script::ScriptError::ValidationFailure(
            format!(
                "by-type-hash/{}",
                ckb_types::H256(rollup_config.custodian_script_type_hash().unpack())
            ),
            ERROR_INVALID_CUSTODIAN_CELL,
        )
        .input_lock_script(input_lock_idx);
        assert_eq!(
            err_msg,
            ckb_error::Error::from(expected_lock_err).to_string()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_rollup_action_cancel_challengel() {
    use super::state_validator::cancel_challenge;

    init_env_log();

    let (mut ctx, tx, rollup_type_script, rollup_config) =
        cancel_challenge::withdrawal::sample_test_case().await;

    let (tx, rollup_config) =
        append_custodian_cells(&mut ctx, tx, rollup_type_script, rollup_config);

    let expected_validator_err = state_validator_script_error(ERROR_INVALID_CUSTODIAN_CELL);
    let input_lock_idx = tx.inputs().len() - 1;

    let err_msg = ctx.verify_tx(tx).unwrap_err().to_string();
    if err_msg != ckb_error::Error::from(expected_validator_err).to_string() {
        let expected_lock_err = ckb_script::ScriptError::ValidationFailure(
            format!(
                "by-type-hash/{}",
                ckb_types::H256(rollup_config.custodian_script_type_hash().unpack())
            ),
            ERROR_INVALID_CUSTODIAN_CELL,
        )
        .input_lock_script(input_lock_idx);
        assert_eq!(
            err_msg,
            ckb_error::Error::from(expected_lock_err).to_string()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_rollup_action_revert() {
    use super::state_validator::revert;

    init_env_log();

    let (mut ctx, tx, rollup_type_script, rollup_config) = revert::sample_test_case().await;

    let (tx, rollup_config) =
        append_custodian_cells(&mut ctx, tx, rollup_type_script, rollup_config);

    let expected_validator_err = state_validator_script_error(ERROR_INVALID_CUSTODIAN_CELL);
    let input_lock_idx = tx.inputs().len() - 1;

    let err_msg = ctx.verify_tx(tx).unwrap_err().to_string();
    if err_msg != ckb_error::Error::from(expected_validator_err).to_string() {
        let expected_lock_err = ckb_script::ScriptError::ValidationFailure(
            format!(
                "by-type-hash/{}",
                ckb_types::H256(rollup_config.custodian_script_type_hash().unpack())
            ),
            ERROR_INVALID_CUSTODIAN_CELL,
        )
        .input_lock_script(input_lock_idx);
        assert_eq!(
            err_msg,
            ckb_error::Error::from(expected_lock_err).to_string()
        );
    }
}
