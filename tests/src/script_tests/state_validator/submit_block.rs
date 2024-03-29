use std::time::{SystemTime, UNIX_EPOCH};

use crate::script_tests::utils::layer1::{
    build_simple_tx_with_out_point_and_since, random_out_point, since_timestamp,
};
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, build_type_id_script,
    calculate_state_validator_type_id, CellContext, CellContextParam,
};
use crate::testing_tool::chain::{build_sync_tx, construct_block_from_timestamp};
use crate::testing_tool::programs::{ALWAYS_SUCCESS_CODE_HASH, STATE_VALIDATOR_CODE_HASH};
use crate::{script_tests::utils::layer1::build_simple_tx, testing_tool::chain::construct_block};
use crate::{
    script_tests::utils::layer1::build_simple_tx_with_out_point, testing_tool::chain::setup_chain,
};
use ckb_error::assert_error_eq;
use ckb_script::ScriptError;
use ckb_types::{
    packed::CellInput,
    prelude::{Pack as CKBPack, Unpack},
};
use gw_chain::chain::{L1Action, L1ActionContext, SyncParam};
use gw_types::core::AllowedEoaType;
use gw_types::packed::{
    AllowedTypeHash, DepositRequest, L2BlockCommittedInfo, RawWithdrawalRequest, WithdrawalRequest,
    WithdrawalRequestExtra,
};
use gw_types::prelude::{Pack as GWPack, Unpack as GWUnpack, *};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        CustodianLockArgs, DepositLockArgs, RollupAction, RollupActionUnion, RollupConfig,
        RollupSubmitBlock, Script, StakeLockArgs, WithdrawalLockArgs,
    },
};

const INVALID_BLOCK_ERROR: i8 = 20;
const INVALID_POST_GLOBAL_STATE: i8 = 23;

#[tokio::test]
async fn test_submit_block() {
    // calculate type id
    let capacity = 1000_00000000u64;
    let spend_cell = build_always_success_cell(capacity, None);
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .build();
    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args.as_bytes(),
        )
    };
    // create a rollup cell
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state.as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (spend_cell, Default::default()),
        input_out_point,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
    )
    .as_advanced_builder()
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");
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
        .version(1u8.into())
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
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        since_timestamp(GWUnpack::unpack(&tip_block_timestamp)),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();
    ctx.verify_tx(tx).expect("return success");
}

#[tokio::test]
async fn test_downgrade_rollup_cell() {
    // calculate type id
    let capacity = 1000_00000000u64;
    let spend_cell = build_always_success_cell(capacity, None);
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .build();
    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args.as_bytes(),
        )
    };
    // create a rollup cell
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state
        .clone()
        .as_builder()
        .version(1u8.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (spend_cell, Default::default()),
        input_out_point,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
    )
    .as_advanced_builder()
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");
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
        .version(0u8.into())
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
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        since_timestamp(GWUnpack::unpack(&tip_block_timestamp)),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-data-hash/{}",
            ckb_types::H256(*STATE_VALIDATOR_CODE_HASH)
        ),
        INVALID_POST_GLOBAL_STATE,
    )
    .input_type_script(0);
    assert_error_eq!(err, expected_err);
}

#[tokio::test]
async fn test_v1_block_timestamp_smaller_or_equal_than_previous_block_in_submit_block() {
    // calculate type id
    let capacity = 1000_00000000u64;
    let spend_cell = build_always_success_cell(capacity, None);
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .build();
    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args.as_bytes(),
        )
    };
    // create a rollup cell
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    let global_state = chain.local_state().last_global_state();
    let initial_timestamp = {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp")
            .as_millis() as u64;
        assert!(timestamp > 100);
        timestamp - 100
    };
    let initial_rollup_cell_data = global_state
        .clone()
        .as_builder()
        .tip_block_timestamp(GWPack::pack(&initial_timestamp))
        .version(1u8.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (spend_cell, Default::default()),
        input_out_point,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
    )
    .as_advanced_builder()
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");

    // #### Submit a smaller block timestamp
    let tip_block_timestamp = initial_timestamp;
    assert!(tip_block_timestamp > 100);
    let block_result = {
        let timestamp = tip_block_timestamp.saturating_sub(100);
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block_from_timestamp(&chain, &mut mem_pool, Vec::default(), timestamp)
            .await
            .unwrap()
    };
    // verify submit block
    let block_timestamp = GWUnpack::unpack(&block_result.block.raw().timestamp());
    assert!(block_timestamp == tip_block_timestamp.saturating_sub(100));
    let rollup_cell_data = {
        let block_timestamp = GWPack::pack(&block_timestamp);
        let builder = block_result.global_state.clone().as_builder();
        builder
            .tip_block_timestamp(block_timestamp)
            .version(1u8.into())
            .build()
    };
    let witness = {
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupSubmitBlock(
                RollupSubmitBlock::new_builder()
                    .block(block_result.block)
                    .build(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
        since_timestamp(tip_block_timestamp.saturating_add(100)),
        (rollup_cell.clone(), rollup_cell_data.as_bytes()),
    )
    .as_advanced_builder()
    .input(input_stake_cell.clone())
    .output(output_stake_cell.clone())
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-data-hash/{}",
            ckb_types::H256(*STATE_VALIDATOR_CODE_HASH)
        ),
        INVALID_BLOCK_ERROR,
    )
    .input_type_script(0);
    assert_error_eq!(err, expected_err);

    // #### Submit a equal block timestamp
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block_from_timestamp(&chain, &mut mem_pool, Vec::default(), tip_block_timestamp)
            .await
            .unwrap()
    };
    // verify submit block
    let block_timestamp = block_result.block.raw().timestamp();
    let rollup_cell_data = block_result
        .global_state
        .clone()
        .as_builder()
        .tip_block_timestamp(block_timestamp)
        .version(1u8.into())
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
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        since_timestamp(tip_block_timestamp.saturating_add(1000)),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-data-hash/{}",
            ckb_types::H256(*STATE_VALIDATOR_CODE_HASH)
        ),
        INVALID_BLOCK_ERROR,
    )
    .input_type_script(0);
    assert_error_eq!(err, expected_err);
}

#[tokio::test]
async fn test_v1_block_timestamp_bigger_than_rollup_input_since_in_submit_block() {
    // calculate type id
    let capacity = 1000_00000000u64;
    let spend_cell = build_always_success_cell(capacity, None);
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .build();
    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args.as_bytes(),
        )
    };
    // create a rollup cell
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state
        .clone()
        .as_builder()
        .version(1u8.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (spend_cell, Default::default()),
        input_out_point,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
    )
    .as_advanced_builder()
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");
    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };
    // verify submit block
    let tip_block_timestamp = GWUnpack::unpack(&block_result.block.raw().timestamp());
    let rollup_cell_data = block_result
        .global_state
        .as_builder()
        .tip_block_timestamp(GWPack::pack(&tip_block_timestamp))
        .version(1u8.into())
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
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    // NOTE: since_timestamp() will increase tip_block_timestamp by 1 second, so we have have to minus 2 seconds
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        since_timestamp(tip_block_timestamp.saturating_sub(2000)),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-data-hash/{}",
            ckb_types::H256(*STATE_VALIDATOR_CODE_HASH)
        ),
        INVALID_BLOCK_ERROR,
    )
    .input_type_script(0);
    assert_error_eq!(err, expected_err);
}

#[tokio::test]
async fn test_v0_v1_wrong_global_state_tip_block_timestamp_in_submit_block() {
    // calculate type id
    let capacity = 1000_00000000u64;
    let spend_cell = build_always_success_cell(capacity, None);
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .build();
    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args.as_bytes(),
        )
    };
    // create a rollup cell
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state
        .clone()
        .as_builder()
        .tip_block_timestamp(GWPack::pack(&0u64))
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (spend_cell, Default::default()),
        input_out_point,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
    )
    .as_advanced_builder()
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");

    // #### Submit a version 0 global state but block timestamp isn't 0
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, Vec::default())
            .await
            .unwrap()
    };
    // verify submit block
    let tip_block_timestamp = GWUnpack::unpack(&block_result.block.raw().timestamp());
    let rollup_cell_data = block_result
        .global_state
        .clone()
        .as_builder()
        .tip_block_timestamp(GWPack::pack(&tip_block_timestamp.saturating_sub(100)))
        .version(0.into())
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
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
        since_timestamp(tip_block_timestamp),
        (rollup_cell.clone(), rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell.clone())
    .output(output_stake_cell.clone())
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-data-hash/{}",
            ckb_types::H256(*STATE_VALIDATOR_CODE_HASH)
        ),
        INVALID_POST_GLOBAL_STATE,
    )
    .input_type_script(0);
    assert_error_eq!(err, expected_err);

    // #### Submit a version 1 global state but wrong block timestamp aka witness block timestamp don't
    // match in global state
    let rollup_cell_data = block_result
        .global_state
        .clone()
        .as_builder()
        .tip_block_timestamp(GWPack::pack(&tip_block_timestamp.saturating_sub(100)))
        .version(1u8.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
        since_timestamp(tip_block_timestamp),
        (rollup_cell.clone(), rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell.clone())
    .output(output_stake_cell.clone())
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-data-hash/{}",
            ckb_types::H256(*STATE_VALIDATOR_CODE_HASH)
        ),
        INVALID_BLOCK_ERROR,
    )
    .input_type_script(0);
    assert_error_eq!(err, expected_err);

    // #### Submit a version 1 global state but block timestamp is bigger than input since
    let rollup_cell_data = block_result
        .global_state
        .as_builder()
        .version(1u8.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        since_timestamp(tip_block_timestamp.saturating_sub(3000)),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-data-hash/{}",
            ckb_types::H256(*STATE_VALIDATOR_CODE_HASH)
        ),
        INVALID_BLOCK_ERROR,
    )
    .input_type_script(0);
    assert_error_eq!(err, expected_err);
}

#[tokio::test]
async fn test_check_reverted_cells_in_submit_block() {
    let capacity = 1000_00000000u64;
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let deposit_lock_type = build_type_id_script(b"deposit_lock_type_id");
    let deposit_script_type_hash: [u8; 32] = deposit_lock_type.calc_script_hash().unpack();
    let custodian_lock_type = build_type_id_script(b"custodian_lock_type_id");
    let custodian_script_type_hash: [u8; 32] = custodian_lock_type.calc_script_hash().unpack();
    let withdrawal_lock_type = build_type_id_script(b"withdrawal_lock_type_id");
    let withdrawal_script_type_hash: [u8; 32] = withdrawal_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .deposit_script_type_hash(Pack::pack(&deposit_script_type_hash))
        .custodian_script_type_hash(Pack::pack(&custodian_script_type_hash))
        .withdrawal_script_type_hash(Pack::pack(&withdrawal_script_type_hash))
        .build();
    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        deposit_lock_type,
        custodian_lock_type,
        withdrawal_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args.as_bytes(),
        )
    };
    // create a rollup cell
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );

    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state
        .clone()
        .as_builder()
        .version(1u8.into())
        .build()
        .as_bytes();
    // build reverted cells inputs and outputs
    let reverted_deposit_capacity: u64 = 200_00000000u64;
    let depositer_lock_script = Script::new_builder()
        .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
        .hash_type(ScriptHashType::Data.into())
        .args(Pack::pack(&Bytes::from(b"sender".to_vec())))
        .build();
    let deposit_args = DepositLockArgs::new_builder()
        .owner_lock_hash(Pack::pack(&[0u8; 32]))
        .layer2_lock(depositer_lock_script)
        .cancel_timeout(Pack::pack(&0))
        .build();
    let revert_block_hash = [42u8; 32];
    let revert_block_number = 2u64;
    // build reverted deposit cell
    let input_reverted_custodian_cell = {
        let args = CustodianLockArgs::new_builder()
            .deposit_lock_args(deposit_args.clone())
            .deposit_block_hash(Pack::pack(&revert_block_hash))
            .deposit_block_number(Pack::pack(&revert_block_number))
            .build();
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &custodian_script_type_hash,
            reverted_deposit_capacity,
            args.as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::new());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_reverted_deposit_cell = {
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &deposit_script_type_hash,
            reverted_deposit_capacity,
            deposit_args.as_bytes(),
        )
    };
    // build reverted withdrawal cell
    let reverted_withdrawal_capacity: u64 = 130_00000000u64;
    let input_reverted_withdrawal_cell = {
        let owner_lock = Script::default();
        let lock_args = WithdrawalLockArgs::new_builder()
            .withdrawal_block_hash(Pack::pack(&revert_block_hash))
            .withdrawal_block_number(Pack::pack(&revert_block_number))
            .owner_lock_hash(Pack::pack(&owner_lock.hash()))
            .build();
        let mut args = Vec::new();
        args.extend_from_slice(&lock_args.as_bytes());
        args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&owner_lock.as_bytes());
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &withdrawal_script_type_hash,
            reverted_withdrawal_capacity,
            args.into(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::new());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_reverted_custodian_cell = {
        let args = CustodianLockArgs::new_builder()
            .deposit_block_hash(Pack::pack(&[0u8; 32]))
            .deposit_block_number(Pack::pack(&0))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &custodian_script_type_hash,
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
                let args = CustodianLockArgs::new_builder()
                    .deposit_block_hash(Pack::pack(&[0u8; 32]))
                    .deposit_block_number(Pack::pack(&0))
                    .build();
                let cell = build_rollup_locked_cell(
                    &rollup_type_script.hash(),
                    &custodian_script_type_hash,
                    capacity,
                    args.as_bytes(),
                );
                let out_point = ctx.insert_cell(cell, Bytes::new());
                CellInput::new_builder().previous_output(out_point).build()
            })
            .collect()
    };
    let output_finalized_cells: Vec<_> = {
        let capacity = 450_00000000u64;
        (0..2)
            .into_iter()
            .map(|_| {
                let args = CustodianLockArgs::new_builder()
                    .deposit_block_hash(Pack::pack(&[0u8; 32]))
                    .deposit_block_number(Pack::pack(&0))
                    .build();
                build_rollup_locked_cell(
                    &rollup_type_script.hash(),
                    &custodian_script_type_hash,
                    capacity,
                    args.as_bytes(),
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
        .version(1u8.into())
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
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = build_simple_tx_with_out_point_and_since(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        (
            input_out_point,
            since_timestamp(GWUnpack::unpack(&tip_block_timestamp)),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .input(input_reverted_custodian_cell)
    .output(output_reverted_deposit_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .input(input_reverted_withdrawal_cell)
    .output(output_reverted_custodian_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .inputs(input_finalized_cells)
    .outputs(output_finalized_cells.clone())
    .outputs_data(
        (0..output_finalized_cells.len())
            .into_iter()
            .map(|_| CKBPack::pack(&Bytes::new())),
    )
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.deposit_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();
    ctx.verify_tx(tx).expect("return success");
}

#[tokio::test]
async fn test_withdrawal_cell_lock_args_with_owner_lock_in_submit_block() {
    let _ = env_logger::builder().is_test(true).try_init();

    let capacity = 1000_00000000u64;
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };

    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let custodian_lock_type = build_type_id_script(b"custodian_lock_type_id");
    let custodian_script_type_hash: [u8; 32] = custodian_lock_type.calc_script_hash().unpack();
    let withdrawal_lock_type = build_type_id_script(b"withdrawal_lock_type_id");
    let withdrawal_script_type_hash: [u8; 32] = withdrawal_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .custodian_script_type_hash(Pack::pack(&custodian_script_type_hash))
        .withdrawal_script_type_hash(Pack::pack(&withdrawal_script_type_hash))
        .allowed_eoa_type_hashes(PackVec::pack(vec![AllowedTypeHash::new(
            AllowedEoaType::Eth,
            *ALWAYS_SUCCESS_CODE_HASH,
        )]))
        .build();

    // setup chain
    let mut chain = setup_chain(rollup_type_script.clone(), rollup_config.clone()).await;

    // create a rollup cell
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    let eth_registry_id = gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;

    // Deposit account
    let deposit_capacity: u64 = 1000000 * 10u64.pow(8);
    let deposit_lock_args = {
        let mut args = rollup_type_script.hash().to_vec();
        args.extend_from_slice(&[1u8; 20]);
        Pack::pack(&Bytes::from(args))
    };
    let account_script = Script::new_builder()
        .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
        .hash_type(ScriptHashType::Type.into())
        .args(deposit_lock_args)
        .build();
    let deposit = DepositRequest::new_builder()
        .capacity(Pack::pack(&deposit_capacity))
        .script(account_script.to_owned())
        .registry_id(Pack::pack(&eth_registry_id))
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
        transaction: build_sync_tx(
            gw_types::packed::CellOutput::new_unchecked(rollup_cell.as_bytes()),
            block_result,
        ),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(Pack::pack(&1u64))
            .build(),
    };
    let param = SyncParam {
        updates: vec![apply_deposits],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    // Withdraw
    let withdrawal = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(Pack::pack(&deposit_capacity))
            .account_script_hash(Pack::pack(&account_script.hash()))
            .owner_lock_hash(Pack::pack(&account_script.hash()))
            .registry_id(Pack::pack(&eth_registry_id))
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

    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        custodian_lock_type,
        withdrawal_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);

    // build stake input and output
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let block_number = block_result.block.raw().number();
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(block_number)
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
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
            &custodian_script_type_hash,
            deposit_capacity,
            CustodianLockArgs::default().as_bytes(),
        );

        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };

    // build withdrawal output
    let output_withdrawal_cell = {
        let lock_args = WithdrawalLockArgs::new_builder()
            .withdrawal_block_number(block_result.block.raw().number())
            .withdrawal_block_hash(Pack::pack(&block_result.block.raw().hash()))
            .account_script_hash(Pack::pack(&account_script.hash()))
            .owner_lock_hash(Pack::pack(&account_script.hash()))
            .build();

        let mut args = lock_args.as_slice().to_vec();
        args.extend_from_slice(&(account_script.as_bytes().len() as u32).to_be_bytes());
        args.extend_from_slice(&account_script.as_bytes());

        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &withdrawal_script_type_hash,
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
        .version(1u8.into())
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
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = build_simple_tx_with_out_point_and_since(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        (
            input_out_point,
            since_timestamp(GWUnpack::unpack(&tip_block_timestamp)),
        ),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .input(input_custodian_cell)
    .output(output_withdrawal_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();
    ctx.verify_tx(tx).expect("return success");
}
