use std::collections::HashSet;

use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::random_out_point;
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, build_type_id_script,
    calculate_state_validator_type_id, CellContext, CellContextParam,
};
use crate::testing_tool::chain::{apply_block_result, construct_block, setup_chain};
use crate::testing_tool::programs::{ALWAYS_SUCCESS_CODE_HASH, STATE_VALIDATOR_CODE_HASH};
use ckb_error::assert_error_eq;
use ckb_script::ScriptError;
use ckb_types::prelude::{Pack as CKBPack, Unpack};
use gw_chain::chain::Chain;
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    state::{to_short_address, State},
};
use gw_store::state_db::SubState;
use gw_store::state_db::{CheckPoint, StateDBMode, StateDBTransaction};
use gw_types::prelude::*;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        ChallengeLockArgs, ChallengeTarget, ChallengeWitness, DepositRequest, L2Transaction,
        RawL2Transaction, RollupAction, RollupActionUnion, RollupConfig, RollupEnterChallenge,
        SUDTArgs, SUDTArgsUnion, SUDTTransfer, Script,
    },
};

const INVALID_CHALLENGE_TARGET_ERROR: i8 = 34;

#[test]
fn test_enter_challenge() {
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
    let challenge_lock_type = build_type_id_script(b"challenge_lock_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let finality_blocks = 10;
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .finality_blocks(Pack::pack(&finality_blocks))
        .build();
    // setup chain
    let mut chain = setup_chain(rollup_type_script.clone(), rollup_config.clone());
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    // produce a block so we can challenge it
    {
        // deposit two account
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(b"sender".to_vec())))
            .build();
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(b"receiver".to_vec())))
            .build();
        let deposit_requests = vec![
            DepositRequest::new_builder()
                .capacity(Pack::pack(&100_00000000u64))
                .script(sender_script.clone())
                .build(),
            DepositRequest::new_builder()
                .capacity(Pack::pack(&50_00000000u64))
                .script(receiver_script.clone())
                .build(),
        ];
        let produce_block_result = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = smol::block_on(mem_pool.lock());
            construct_block(&chain, &mut mem_pool, deposit_requests.clone()).unwrap()
        };
        let rollup_cell = gw_types::packed::CellOutput::new_unchecked(rollup_cell.as_bytes());
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            rollup_cell.clone(),
            produce_block_result,
            deposit_requests,
            asset_scripts,
        );
        let db = chain.store().begin_transaction();
        let tip_block = db.get_tip_block().unwrap();
        let tip_block_number = gw_types::prelude::Unpack::unpack(&tip_block.raw().number());
        let state_db = StateDBTransaction::from_checkpoint(
            &db,
            CheckPoint::new(tip_block_number, SubState::Block),
            StateDBMode::ReadOnly,
        )
        .unwrap();
        let tree = state_db.state_tree().unwrap();
        let sender_id = tree
            .get_account_id_by_script_hash(&sender_script.hash().into())
            .unwrap()
            .unwrap();
        let receiver_id = tree
            .get_account_id_by_script_hash(&receiver_script.hash().into())
            .unwrap()
            .unwrap();
        let receiver_script_hash = tree.get_script_hash(receiver_id).expect("get script hash");
        let receiver_address = Bytes::copy_from_slice(to_short_address(&receiver_script_hash));
        let produce_block_result = {
            let args = SUDTArgs::new_builder()
                .set(SUDTArgsUnion::SUDTTransfer(
                    SUDTTransfer::new_builder()
                        .amount(Pack::pack(&50_00000000u128))
                        .to(Pack::pack(&receiver_address))
                        .build(),
                ))
                .build()
                .as_bytes();
            let tx = L2Transaction::new_builder()
                .raw(
                    RawL2Transaction::new_builder()
                        .from_id(Pack::pack(&sender_id))
                        .to_id(Pack::pack(&CKB_SUDT_ACCOUNT_ID))
                        .nonce(Pack::pack(&0u32))
                        .args(Pack::pack(&args))
                        .build(),
                )
                .build();
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = smol::block_on(mem_pool.lock());
            mem_pool.push_transaction(tx).unwrap();
            construct_block(&chain, &mut mem_pool, Vec::default()).unwrap()
        };
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            rollup_cell,
            produce_block_result,
            vec![],
            asset_scripts,
        );
    }
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type: stake_lock_type.clone(),
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let challenged_block = chain.local_state().tip().clone();
    let challenge_capacity = 10000_00000000u64;
    let challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&0u32))
                    .target_type(ChallengeTargetType::TxExecution.into())
                    .block_hash(Pack::pack(&challenged_block.hash()))
                    .build(),
            )
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &challenge_script_type_hash,
            challenge_capacity,
            lock_args.as_bytes(),
        )
    };
    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state.as_bytes();
    // verify enter challenge
    let witness = {
        let block_proof: Bytes = {
            let db = chain.store().begin_transaction();
            let proof = db
                .block_smt()
                .unwrap()
                .merkle_proof(vec![challenged_block.smt_key().into()])
                .unwrap();
            proof
                .compile(vec![(
                    challenged_block.smt_key().into(),
                    challenged_block.hash().into(),
                )])
                .unwrap()
                .0
                .into()
        };
        let witness = ChallengeWitness::new_builder()
            .raw_l2block(challenged_block.raw())
            .block_proof(Pack::pack(&block_proof))
            .build();
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupEnterChallenge(
                RollupEnterChallenge::new_builder().witness(witness).build(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let rollup_cell_data = global_state
        .clone()
        .as_builder()
        .status(Status::Halting.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        input_out_point,
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .output(challenge_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();
    ctx.verify_tx(tx).expect("return success");
}

#[test]
fn test_enter_challenge_finalized_block() {
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
    let challenge_lock_type = build_type_id_script(b"challenge_lock_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let finality_blocks = 1;
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .finality_blocks(Pack::pack(&finality_blocks))
        .build();
    // setup chain
    let mut chain = setup_chain(rollup_type_script.clone(), rollup_config.clone());
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );

    // deposit two account
    let (sender_id, receiver_address) = {
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(b"sender".to_vec())))
            .build();
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(b"receiver".to_vec())))
            .build();
        let deposit_requests = vec![
            DepositRequest::new_builder()
                .capacity(Pack::pack(&100_00000000u64))
                .script(sender_script.clone())
                .build(),
            DepositRequest::new_builder()
                .capacity(Pack::pack(&50_00000000u64))
                .script(receiver_script.clone())
                .build(),
        ];
        let produce_block_result = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = smol::block_on(mem_pool.lock());
            construct_block(&chain, &mut mem_pool, deposit_requests.clone()).unwrap()
        };
        let rollup_cell = gw_types::packed::CellOutput::new_unchecked(rollup_cell.as_bytes());
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            rollup_cell.clone(),
            produce_block_result,
            deposit_requests,
            asset_scripts,
        );
        let db = chain.store().begin_transaction();
        let tip_block = db.get_tip_block().unwrap();
        let tip_block_number = gw_types::prelude::Unpack::unpack(&tip_block.raw().number());
        let state_db = StateDBTransaction::from_checkpoint(
            &db,
            CheckPoint::new(tip_block_number, SubState::Block),
            StateDBMode::ReadOnly,
        )
        .unwrap();
        let tree = state_db.state_tree().unwrap();
        let sender_id = tree
            .get_account_id_by_script_hash(&sender_script.hash().into())
            .unwrap()
            .unwrap();
        let receiver_id = tree
            .get_account_id_by_script_hash(&receiver_script.hash().into())
            .unwrap()
            .unwrap();
        let receiver_script_hash = tree.get_script_hash(receiver_id).expect("get script hash");
        let receiver_address = Bytes::copy_from_slice(to_short_address(&receiver_script_hash));

        (sender_id, receiver_address)
    };

    let produce_block = |chain: &mut Chain, nonce: u32| {
        let rollup_cell = gw_types::packed::CellOutput::new_unchecked(rollup_cell.as_bytes());
        let produce_block_result = {
            let args = SUDTArgs::new_builder()
                .set(SUDTArgsUnion::SUDTTransfer(
                    SUDTTransfer::new_builder()
                        .amount(Pack::pack(&50_00000000u128))
                        .to(Pack::pack(&receiver_address))
                        .build(),
                ))
                .build()
                .as_bytes();
            let tx = L2Transaction::new_builder()
                .raw(
                    RawL2Transaction::new_builder()
                        .from_id(Pack::pack(&sender_id))
                        .to_id(Pack::pack(&CKB_SUDT_ACCOUNT_ID))
                        .nonce(Pack::pack(&nonce))
                        .args(Pack::pack(&args))
                        .build(),
                )
                .build();
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = smol::block_on(mem_pool.lock());
            mem_pool.push_transaction(tx).unwrap();
            construct_block(&chain, &mut mem_pool, Vec::default()).unwrap()
        };
        let asset_scripts = HashSet::new();
        apply_block_result(
            chain,
            rollup_cell,
            produce_block_result,
            vec![],
            asset_scripts,
        );
    };

    // produce two blocks and challenge first one
    let mut nonce = 0u32;
    produce_block(&mut chain, nonce);
    nonce += 1;

    let challenged_block = chain.local_state().tip().clone();
    produce_block(&mut chain, nonce); // Make first block finalized

    // deploy scripts
    let param = CellContextParam {
        stake_lock_type: stake_lock_type.clone(),
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let challenge_capacity = 10000_00000000u64;
    let challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&0u32))
                    .target_type(ChallengeTargetType::TxExecution.into())
                    .block_hash(Pack::pack(&challenged_block.hash()))
                    .build(),
            )
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &challenge_script_type_hash,
            challenge_capacity,
            lock_args.as_bytes(),
        )
    };
    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state.as_bytes();

    // verify enter challenge
    let witness = {
        let block_proof: Bytes = {
            let db = chain.store().begin_transaction();
            let proof = db
                .block_smt()
                .unwrap()
                .merkle_proof(vec![challenged_block.smt_key().into()])
                .unwrap();
            proof
                .compile(vec![(
                    challenged_block.smt_key().into(),
                    challenged_block.hash().into(),
                )])
                .unwrap()
                .0
                .into()
        };
        let witness = ChallengeWitness::new_builder()
            .raw_l2block(challenged_block.raw())
            .block_proof(Pack::pack(&block_proof))
            .build();
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupEnterChallenge(
                RollupEnterChallenge::new_builder().witness(witness).build(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let rollup_cell_data = global_state
        .clone()
        .as_builder()
        .status(Status::Halting.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        input_out_point,
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .output(challenge_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err =
        ScriptError::ValidationFailure(INVALID_CHALLENGE_TARGET_ERROR).input_type_script(0);
    assert_error_eq!(err, expected_err);
}
