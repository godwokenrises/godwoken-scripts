use crate::script_tests::state_validator::finalize_withdrawal::{
    BLOCK_ALL_WITHDRAWALS, BLOCK_NO_WITHDRAWAL,
};
use crate::script_tests::utils::conversion::{CKBTypeIntoExt, ToCKBType, ToGWType};
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::{
    build_simple_tx_with_out_point_and_since, random_out_point, since_timestamp,
    state_validator_script_error,
};
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, build_type_id_script,
    calculate_state_validator_type_id, CellContext, CellContextParam,
};
use crate::testing_tool::chain::build_sync_tx;
use crate::testing_tool::chain::construct_block;
use crate::testing_tool::chain::setup_chain;
use crate::testing_tool::programs::{ALWAYS_SUCCESS_CODE_HASH, STATE_VALIDATOR_CODE_HASH};
use ckb_error::assert_error_eq;
use gw_chain::chain::{Chain, L1Action, L1ActionContext, SyncParam};
use gw_types::core::AllowedEoaType;
use gw_types::packed::{
    AllowedTypeHash, CellInput, DepositRequest, L2BlockCommittedInfo, LastFinalizedWithdrawal,
    RawWithdrawalRequest, WithdrawalRequest, WithdrawalRequestExtra, WitnessArgs,
};
use gw_types::prelude::{Builder, Entity, Pack, PackVec, Unpack};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        CustodianLockArgs, RollupAction, RollupActionUnion, RollupConfig, RollupSubmitBlock,
        Script, StakeLockArgs, WithdrawalLockArgs,
    },
};

const ERROR_INVALID_POST_GLOBAL_STATE: i8 = 23;
const ERROR_INVALID_WITHDRAWAL_CELL: i8 = 27;
const ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL: i8 = 46;

// For global state version < 2
#[tokio::test]
async fn test_non_default_last_finalized_withdrawal_in_prev_global_state() {
    init_env_log();

    let TestEnv {
        rollup_type_script,
        rollup_config,
        chain,
        account_script,
        deposit_capacity,
        eth_registry_id,
        cell_context: mut ctx,
        rollup_cell,
        rollup_outpoint,
    } = setup_test_env().await;

    // Withdraw
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(deposit_capacity.pack())
            .account_script_hash(account_script.hash().pack())
            .owner_lock_hash(account_script.hash().pack())
            .registry_id(eth_registry_id.pack())
            .build();
        let request = WithdrawalRequest::new_builder().raw(raw).build();
        WithdrawalRequestExtra::new_builder()
            .request(request)
            .owner_lock(account_script.clone())
            .build()
    };

    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        mem_pool.reset_mem_block().await.unwrap();
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };
    assert_eq!(block_result.block.withdrawals().len(), 1);

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
    let non_default_last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
        .block_number(1.pack())
        .build();
    let initial_rollup_cell_data = global_state
        .clone()
        .as_builder()
        .last_finalized_withdrawal(non_default_last_finalized_withdrawal)
        .version(1u8.into())
        .build()
        .as_bytes();

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
            rollup_outpoint,
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(Bytes::default().to_ckb())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(witness.as_bytes().to_ckb())
    .build();

    let expected_err = state_validator_script_error(ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL);
    let err = ctx.verify_tx(tx).unwrap_err();
    assert_error_eq!(err, expected_err);
}

#[tokio::test]
async fn test_modify_last_finalized_withdrawal() {
    init_env_log();

    let TestEnv {
        rollup_type_script,
        rollup_config,
        chain,
        account_script,
        deposit_capacity,
        eth_registry_id,
        cell_context: mut ctx,
        rollup_cell,
        rollup_outpoint,
    } = setup_test_env().await;

    // Withdraw
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(deposit_capacity.pack())
            .account_script_hash(account_script.hash().pack())
            .owner_lock_hash(account_script.hash().pack())
            .registry_id(eth_registry_id.pack())
            .build();
        let request = WithdrawalRequest::new_builder().raw(raw).build();
        WithdrawalRequestExtra::new_builder()
            .request(request)
            .owner_lock(account_script.clone())
            .build()
    };

    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        mem_pool.reset_mem_block().await.unwrap();
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };
    assert_eq!(block_result.block.withdrawals().len(), 1);

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

    // verify submit block
    let tip_block_timestamp = block_result.block.raw().timestamp();
    let updated_last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
        .block_number(1.pack())
        .build();
    let rollup_cell_data = block_result
        .global_state
        .as_builder()
        .tip_block_timestamp(tip_block_timestamp.clone())
        .last_finalized_withdrawal(updated_last_finalized_withdrawal)
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
            rollup_outpoint,
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(Bytes::default().to_ckb())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(witness.as_bytes().to_ckb())
    .build();

    let expected_err = state_validator_script_error(ERROR_INVALID_POST_GLOBAL_STATE);
    let err = ctx.verify_tx(tx).unwrap_err();
    assert_error_eq!(err, expected_err);
}

#[tokio::test]
async fn test_upgrade_to_v2_post_global_state_block_no_withdrawals() {
    init_env_log();

    let TestEnv {
        rollup_type_script,
        rollup_config,
        chain,
        account_script: _,
        deposit_capacity: _,
        eth_registry_id: _,
        cell_context: mut ctx,
        rollup_cell,
        rollup_outpoint,
    } = setup_test_env().await;

    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.reset_mem_block().await.unwrap();
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
        .version(1u8.into())
        .build()
        .as_bytes();

    // verify submit block
    let tip_block_timestamp = block_result.block.raw().timestamp();
    let last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
        .block_number(block_result.block.raw().number())
        .withdrawal_index(BLOCK_NO_WITHDRAWAL.pack())
        .build();
    let rollup_cell_data = block_result
        .global_state
        .as_builder()
        .tip_block_timestamp(tip_block_timestamp.clone())
        .last_finalized_withdrawal(last_finalized_withdrawal)
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
            rollup_outpoint,
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
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

#[tokio::test]
async fn test_upgrade_to_v2_post_global_state_block_all_withdrawals() {
    init_env_log();

    let TestEnv {
        rollup_type_script,
        rollup_config,
        chain,
        account_script,
        deposit_capacity,
        eth_registry_id,
        cell_context: mut ctx,
        rollup_cell,
        rollup_outpoint,
    } = setup_test_env().await;

    // Withdraw
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(deposit_capacity.pack())
            .account_script_hash(account_script.hash().pack())
            .owner_lock_hash(account_script.hash().pack())
            .registry_id(eth_registry_id.pack())
            .build();
        let request = WithdrawalRequest::new_builder().raw(raw).build();
        WithdrawalRequestExtra::new_builder()
            .request(request)
            .owner_lock(account_script.clone())
            .build()
    };

    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        mem_pool.reset_mem_block().await.unwrap();
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };
    assert_eq!(block_result.block.withdrawals().len(), 1);

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
        .version(1u8.into())
        .build()
        .as_bytes();

    // build custodian input
    let input_custodian_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.custodian_script_type_hash().unpack(),
            deposit_capacity,
            CustodianLockArgs::default().as_bytes(),
        );

        ctx.insert_cell(cell, Bytes::default()).into_ext()
    };

    // build withdrawal output
    let output_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .withdrawal_block_number(block_result.block.raw().number())
            .withdrawal_block_hash(block_result.block.raw().hash().pack())
            .account_script_hash(account_script.hash().pack())
            .owner_lock_hash(account_script.hash().pack())
            .build();

        let mut args = lock_args.as_slice().to_vec();
        args.extend_from_slice(&(account_script.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&account_script.as_bytes());

        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.withdrawal_script_type_hash().unpack(),
            deposit_capacity,
            Bytes::from(args),
        )
    };

    // verify submit block
    let tip_block_timestamp = block_result.block.raw().timestamp();
    let last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
        .block_number(block_result.block.raw().number())
        .withdrawal_index(BLOCK_ALL_WITHDRAWALS.pack())
        .build();
    let rollup_cell_data = block_result
        .global_state
        .as_builder()
        .tip_block_timestamp(tip_block_timestamp.clone())
        .last_finalized_withdrawal(last_finalized_withdrawal)
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
            rollup_outpoint,
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(Bytes::default().to_ckb())
    .input(input_custodian_cell)
    .output(output_withdrawal_cell)
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

#[tokio::test]
async fn test_upgrade_to_v2_post_global_state_wrong_last_finalized_withdrawal() {
    init_env_log();

    let TestEnv {
        rollup_type_script,
        rollup_config,
        chain,
        account_script: _,
        deposit_capacity: _,
        eth_registry_id: _,
        cell_context: mut ctx,
        rollup_cell,
        rollup_outpoint,
    } = setup_test_env().await;

    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.reset_mem_block().await.unwrap();
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };

    // build stake input and output
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell: ckb_types::packed::CellInput = {
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
        .version(1u8.into())
        .build()
        .as_bytes();

    // verify submit block (unchanged)
    let tip_block_timestamp = block_result.block.raw().timestamp();
    let rollup_cell_data = block_result
        .global_state
        .clone()
        .as_builder()
        .tip_block_timestamp(tip_block_timestamp.clone())
        .version(2u8.into())
        .build()
        .as_bytes();
    let witness = {
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupSubmitBlock(
                RollupSubmitBlock::new_builder()
                    .block(block_result.block.clone())
                    .build(),
            ))
            .build();
        WitnessArgs::new_builder()
            .output_type(Some(rollup_action.as_bytes()).pack())
            .build()
    };
    let tx = build_simple_tx_with_out_point_and_since(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
        (
            rollup_outpoint.clone(),
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell.clone(), rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell.clone())
    .output(output_stake_cell.clone())
    .output_data(Bytes::default().to_ckb())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(witness.as_bytes().to_ckb())
    .build();

    let expected_err = state_validator_script_error(ERROR_INVALID_POST_GLOBAL_STATE);
    let err = ctx.verify_tx(tx).unwrap_err();
    assert_error_eq!(err, expected_err);

    // verify submit block (invalid block number)
    let tip_block_timestamp = block_result.block.raw().timestamp();
    let last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
        .block_number(block_result.block.raw().number())
        .withdrawal_index(BLOCK_ALL_WITHDRAWALS.pack())
        .build();
    let rollup_cell_data = block_result
        .global_state
        .clone()
        .as_builder()
        .tip_block_timestamp(tip_block_timestamp.clone())
        .last_finalized_withdrawal(last_finalized_withdrawal)
        .version(2u8.into())
        .build()
        .as_bytes();
    let witness = {
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupSubmitBlock(
                RollupSubmitBlock::new_builder()
                    .block(block_result.block.clone())
                    .build(),
            ))
            .build();
        WitnessArgs::new_builder()
            .output_type(Some(rollup_action.as_bytes()).pack())
            .build()
    };
    let tx = build_simple_tx_with_out_point_and_since(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
        (
            rollup_outpoint.clone(),
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell.clone(), rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell.clone())
    .output(output_stake_cell.clone())
    .output_data(Bytes::default().to_ckb())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(witness.as_bytes().to_ckb())
    .build();

    let expected_err = state_validator_script_error(ERROR_INVALID_POST_GLOBAL_STATE);
    let err = ctx.verify_tx(tx).unwrap_err();
    assert_error_eq!(err, expected_err);

    // verify submit block (invalid withdrawal index)
    let tip_block_timestamp = block_result.block.raw().timestamp();
    let last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
        .block_number(block_result.block.raw().number())
        .withdrawal_index(BLOCK_ALL_WITHDRAWALS.pack())
        .build();
    let rollup_cell_data = block_result
        .global_state
        .as_builder()
        .tip_block_timestamp(tip_block_timestamp.clone())
        .last_finalized_withdrawal(last_finalized_withdrawal)
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
            rollup_outpoint.clone(),
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(Bytes::default().to_ckb())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(witness.as_bytes().to_ckb())
    .build();

    let expected_err = state_validator_script_error(ERROR_INVALID_POST_GLOBAL_STATE);
    let err = ctx.verify_tx(tx).unwrap_err();
    assert_error_eq!(err, expected_err);
}

#[tokio::test]
async fn test_no_output_withdrawal_cell() {
    init_env_log();

    let TestEnv {
        rollup_type_script,
        rollup_config,
        chain,
        account_script,
        deposit_capacity,
        eth_registry_id,
        cell_context: mut ctx,
        rollup_cell,
        rollup_outpoint,
    } = setup_test_env().await;

    // Withdraw
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(deposit_capacity.pack())
            .account_script_hash(account_script.hash().pack())
            .owner_lock_hash(account_script.hash().pack())
            .registry_id(eth_registry_id.pack())
            .build();
        let request = WithdrawalRequest::new_builder().raw(raw).build();
        WithdrawalRequestExtra::new_builder()
            .request(request)
            .owner_lock(account_script.clone())
            .build()
    };

    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        mem_pool.reset_mem_block().await.unwrap();
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };
    assert_eq!(block_result.block.withdrawals().len(), 1);

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
            deposit_capacity,
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
            deposit_capacity,
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
            rollup_outpoint,
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

#[tokio::test]
async fn test_output_withdrawal_cell_found() {
    init_env_log();

    let TestEnv {
        rollup_type_script,
        rollup_config,
        chain,
        account_script,
        deposit_capacity,
        eth_registry_id,
        cell_context: mut ctx,
        rollup_cell,
        rollup_outpoint,
    } = setup_test_env().await;

    // Withdraw
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(deposit_capacity.pack())
            .account_script_hash(account_script.hash().pack())
            .owner_lock_hash(account_script.hash().pack())
            .registry_id(eth_registry_id.pack())
            .build();
        let request = WithdrawalRequest::new_builder().raw(raw).build();
        WithdrawalRequestExtra::new_builder()
            .request(request)
            .owner_lock(account_script.clone())
            .build()
    };

    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        mem_pool.reset_mem_block().await.unwrap();
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };
    assert_eq!(block_result.block.withdrawals().len(), 1);

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
            deposit_capacity,
            CustodianLockArgs::default().as_bytes(),
        );

        ctx.insert_cell(cell, Bytes::default()).into_ext()
    };

    // build withdrawal output
    let output_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .withdrawal_block_number(block_result.block.raw().number())
            .withdrawal_block_hash(block_result.block.raw().hash().pack())
            .account_script_hash(account_script.hash().pack())
            .owner_lock_hash(account_script.hash().pack())
            .build();

        let mut args = lock_args.as_slice().to_vec();
        args.extend_from_slice(&(account_script.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&account_script.as_bytes());

        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.withdrawal_script_type_hash().unpack(),
            deposit_capacity,
            Bytes::from(args),
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
            rollup_outpoint,
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(Bytes::default().to_ckb())
    .input(input_custodian_cell)
    .output(output_withdrawal_cell)
    .output_data(Bytes::default().to_ckb())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(witness.as_bytes().to_ckb())
    .build();

    let expected_err = state_validator_script_error(ERROR_INVALID_WITHDRAWAL_CELL);
    let err = ctx.verify_tx(tx).unwrap_err();
    assert_error_eq!(err, expected_err);
}

#[tokio::test]
async fn test_input_reverted_withdrawal_cell_found() {
    init_env_log();

    let TestEnv {
        rollup_type_script,
        rollup_config,
        chain,
        account_script: _,
        deposit_capacity: _,
        eth_registry_id: _,
        cell_context: mut ctx,
        rollup_cell,
        rollup_outpoint,
    } = setup_test_env().await;

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
        let local_tip_block_number = chain.local_state().tip().raw().number().unpack();
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number((local_tip_block_number + 1).pack())
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
    // build reverted cells inputs and outputs
    let revert_block_hash = [42u8; 32];
    let revert_block_number = 2u64;
    // build reverted withdrawal cell
    let reverted_withdrawal_capacity: u64 = 130_00000000u64;
    let input_reverted_withdrawal_cell = {
        let owner_lock = Script::default();
        let lock_args = WithdrawalLockArgs::new_builder()
            .withdrawal_block_hash(revert_block_hash.pack())
            .withdrawal_block_number(revert_block_number.pack())
            .owner_lock_hash(owner_lock.hash().pack())
            .build();
        let mut args = Vec::new();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.withdrawal_script_type_hash().unpack(),
            reverted_withdrawal_capacity,
            args.into(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::new());
        CellInput::new_builder()
            .previous_output(out_point.to_gw())
            .build()
    };
    let output_reverted_custodian_cell = {
        let args = CustodianLockArgs::new_builder()
            .deposit_block_hash([0u8; 32].pack())
            .deposit_block_number(0.pack())
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &rollup_config.custodian_script_type_hash().unpack(),
            reverted_withdrawal_capacity,
            args.as_bytes(),
        )
    };
    // build arbitrary inputs & outputs finalized custodian cell
    // simulate merge & split finalized custodian cells
    let input_finalized_cells: Vec<_> = {
        let capacity = 300_00000000u64;
        (0..3)
            .into_iter()
            .map(|_| {
                let cell = build_rollup_locked_cell(
                    &rollup_type_script.hash(),
                    &rollup_config.custodian_script_type_hash().unpack(),
                    capacity,
                    CustodianLockArgs::default().as_bytes(),
                );
                ctx.insert_cell(cell, Bytes::new()).into_ext()
            })
            .collect()
    };
    let output_finalized_cells: Vec<_> = {
        let capacity = 450_00000000u64;
        (0..2)
            .into_iter()
            .map(|_| {
                build_rollup_locked_cell(
                    &rollup_type_script.hash(),
                    &rollup_config.custodian_script_type_hash().unpack(),
                    capacity,
                    CustodianLockArgs::default().as_bytes(),
                )
            })
            .collect()
    };
    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
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
            rollup_outpoint,
            since_timestamp(tip_block_timestamp.unpack()),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(Bytes::default().to_ckb())
    .input(input_reverted_withdrawal_cell.to_ckb())
    .output(output_reverted_custodian_cell)
    .output_data(Bytes::default().to_ckb())
    .inputs(input_finalized_cells)
    .outputs(output_finalized_cells.clone())
    .outputs_data(
        (0..output_finalized_cells.len())
            .into_iter()
            .map(|_| Bytes::new().to_ckb()),
    )
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.deposit_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(witness.as_bytes().to_ckb())
    .build();

    let expected_err = state_validator_script_error(ERROR_INVALID_WITHDRAWAL_CELL);
    let err = ctx.verify_tx(tx).unwrap_err();
    assert_error_eq!(err, expected_err);
}

struct TestEnv {
    rollup_type_script: Script,
    rollup_config: RollupConfig,

    chain: Chain,

    account_script: Script,
    deposit_capacity: u64,
    eth_registry_id: u32,

    cell_context: CellContext,
    rollup_cell: ckb_types::packed::CellOutput,
    rollup_outpoint: ckb_types::packed::OutPoint,
}

async fn setup_test_env() -> TestEnv {
    let capacity = 1000_00000000u64;
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
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
    let mut chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;

    // create a rollup cell
    let rollup_cell = build_always_success_cell(capacity, Some(rollup_type_script.to_ckb()));

    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        custodian_lock_type,
        withdrawal_lock_type,
        ..Default::default()
    };
    let cell_context = CellContext::new(&rollup_config, param);

    let eth_registry_id = gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;

    // Deposit account
    let deposit_capacity: u64 = 1000000 * 10u64.pow(8);
    let deposit_lock_args = {
        let mut args = rollup_type_script.hash().to_vec();
        args.extend_from_slice(&[1u8; 20]);
        Bytes::from(args).pack()
    };
    let account_script = Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args(deposit_lock_args)
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity(deposit_capacity.pack())
        .script(account_script.to_owned())
        .registry_id(eth_registry_id.pack())
        .build();

    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, vec![deposit.clone()])
            .await
            .unwrap()
    };
    let apply_deposits = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: vec![deposit],
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.to_gw(), block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![apply_deposits],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    TestEnv {
        rollup_type_script,
        rollup_config,

        chain,

        account_script,
        deposit_capacity,
        eth_registry_id,

        cell_context,
        rollup_cell,
        rollup_outpoint: input_out_point,
    }
}
