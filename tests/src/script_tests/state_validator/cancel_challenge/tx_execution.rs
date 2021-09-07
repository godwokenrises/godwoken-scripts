use std::collections::HashSet;

use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::random_out_point;
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, build_type_id_script,
    calculate_state_validator_type_id, CellContext, CellContextParam,
};
use crate::testing_tool::chain::setup_chain;
use crate::testing_tool::chain::{apply_block_result, construct_block};
use crate::testing_tool::programs::{ALWAYS_SUCCESS_CODE_HASH, STATE_VALIDATOR_CODE_HASH};
use ckb_types::{
    packed::{CellInput, CellOutput},
    prelude::{Pack as CKBPack, Unpack},
};
use gw_common::merkle_utils::ckb_merkle_leaf_hash;
use gw_common::{
    state::{to_short_address, State},
    H256,
};
use gw_store::state_db::SubState;
use gw_store::state_db::{CheckPoint, StateDBMode, StateDBTransaction};
use gw_traits::CodeStore;
use gw_types::prelude::*;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        BlockHashEntry, BlockHashEntryVec, Byte32, ChallengeLockArgs, ChallengeTarget,
        DepositRequest, L2Transaction, RawL2Transaction, RollupAction, RollupActionUnion,
        RollupCancelChallenge, RollupConfig, SUDTArgs, SUDTTransfer, Script, ScriptVec,
        VerifyTransactionContext, VerifyTransactionWitness,
    },
};

#[test]
fn test_cancel_tx_execute() {
    init_env_log();
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
    let eoa_lock_type = build_type_id_script(b"eoa_lock_type_id");
    let l2_sudt_type = build_type_id_script(b"l2_sudt_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let eoa_lock_type_hash: [u8; 32] = eoa_lock_type.calc_script_hash().unpack();
    let l2_sudt_type_hash: [u8; 32] = l2_sudt_type.calc_script_hash().unpack();

    let finality_blocks = 10;
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .l2_sudt_validator_script_type_hash(Pack::pack(&l2_sudt_type_hash))
        .allowed_contract_type_hashes(PackVec::pack(vec![Pack::pack(&l2_sudt_type_hash)]))
        .finality_blocks(Pack::pack(&finality_blocks))
        .allowed_eoa_type_hashes(vec![*ALWAYS_SUCCESS_CODE_HASH].pack())
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
    // CKB built-in account id
    let sudt_id = 1;
    let rollup_script_hash = rollup_type_script.hash();
    // produce a block so we can challenge it
    let (sender_script, receiver_script, sudt_script) = {
        // deposit two account
        let mut sender_args = rollup_script_hash.to_vec();
        sender_args.extend_from_slice(b"sender");
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(sender_args)))
            .build();
        let mut receiver_args = rollup_script_hash.to_vec();
        receiver_args.extend_from_slice(b"receiver");
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(receiver_args)))
            .build();
        let deposit_requests = vec![
            DepositRequest::new_builder()
                .capacity(Pack::pack(&300_00000000u64))
                .script(sender_script.clone())
                .build(),
            DepositRequest::new_builder()
                .capacity(Pack::pack(&150_00000000u64))
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
        let sudt_script_hash = tree.get_script_hash(sudt_id).unwrap();
        let sudt_script = tree.get_script(&sudt_script_hash).unwrap();
        let transfer_capacity = 150_00000000u128;
        let fee_capacity = 1_00000000u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(Pack::pack(&receiver_address))
                    .amount(Pack::pack(&transfer_capacity))
                    .fee(Pack::pack(&fee_capacity))
                    .build(),
            )
            .build()
            .as_bytes();
        let tx = L2Transaction::new_builder()
            .raw(
                RawL2Transaction::new_builder()
                    .from_id(Pack::pack(&sender_id))
                    .to_id(Pack::pack(&sudt_id))
                    .nonce(Pack::pack(&0u32))
                    .args(Pack::pack(&args))
                    .build(),
            )
            .build();
        let produce_block_result = {
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
        (sender_script, receiver_script, sudt_script)
    };
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type: stake_lock_type.clone(),
        challenge_lock_type: challenge_lock_type.clone(),
        eoa_lock_type: eoa_lock_type.clone(),
        l2_sudt_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let challenge_capacity = 10000_00000000u64;
    let challenged_block = chain.local_state().tip().clone();
    let challenge_target_index = 0u32;
    let input_challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&challenge_target_index))
                    .target_type(ChallengeTargetType::TxExecution.into())
                    .block_hash(Pack::pack(&challenged_block.hash()))
                    .build(),
            )
            .build();
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &challenge_script_type_hash,
            challenge_capacity,
            lock_args.as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::new());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let global_state = chain
        .local_state()
        .last_global_state()
        .clone()
        .as_builder()
        .status(Status::Halting.into())
        .build();
    let initial_rollup_cell_data = global_state.as_bytes();
    // verify enter challenge
    let witness = {
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupCancelChallenge(
                RollupCancelChallenge::default(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = challenged_block
        .transactions()
        .get(challenge_target_index as usize)
        .unwrap();
    let challenge_witness = {
        let witness = {
            let leaves: Vec<H256> = challenged_block
                .transactions()
                .into_iter()
                .enumerate()
                .map(|(idx, tx)| ckb_merkle_leaf_hash(idx as u32, &tx.witness_hash().into()))
                .collect();
            let tx_proof = super::build_merkle_proof(&leaves, &[challenge_target_index]);
            let challenged_block_number =
                gw_types::prelude::Unpack::unpack(&challenged_block.raw().number());
            let db = chain.store().begin_transaction();
            let state_db = StateDBTransaction::from_checkpoint(
                &db,
                CheckPoint::new(challenged_block_number - 1, SubState::Block),
                StateDBMode::ReadOnly,
            )
            .unwrap();
            let mut tree = state_db.state_tree().unwrap();
            tree.tracker_mut().enable();
            let sender_id = tree
                .get_account_id_by_script_hash(&sender_script.hash().into())
                .unwrap()
                .unwrap();
            tree.get_script_hash(sender_id).unwrap();
            tree.get_nonce(sender_id).unwrap();
            let receiver_id = tree
                .get_account_id_by_script_hash(&receiver_script.hash().into())
                .unwrap()
                .unwrap();
            tree.get_script_hash(receiver_id).unwrap();
            tree.get_nonce(receiver_id).unwrap();
            tree.get_script_hash(sudt_id).unwrap();
            let account_count = tree.get_account_count().unwrap();
            let touched_keys: Vec<H256> = tree
                .tracker_mut()
                .touched_keys()
                .unwrap()
                .borrow()
                .clone()
                .into_iter()
                .collect();
            let kv_state = touched_keys
                .iter()
                .map(|k| {
                    let v = tree.get_raw(k).unwrap();
                    (*k, v)
                })
                .collect::<Vec<(H256, H256)>>();

            let kv_state_proof: Bytes = {
                let smt = state_db.account_smt().unwrap();
                smt.merkle_proof(touched_keys)
                    .unwrap()
                    .compile(kv_state.clone())
                    .unwrap()
                    .0
                    .into()
            };
            let block_hashes = BlockHashEntryVec::new_builder()
                .push(
                    BlockHashEntry::new_builder()
                        .number(challenged_block.raw().number())
                        .hash(Pack::pack(&challenged_block.hash()))
                        .build(),
                )
                .build();
            let block_hashes_proof: Bytes = {
                let smt = db.block_smt().unwrap();
                smt.merkle_proof(vec![challenged_block.smt_key().into()])
                    .unwrap()
                    .compile(vec![(
                        challenged_block.smt_key().into(),
                        challenged_block.hash().into(),
                    )])
                    .unwrap()
                    .0
                    .into()
            };
            let return_data_hash = [42u8; 32];
            let context = VerifyTransactionContext::new_builder()
                .scripts(
                    ScriptVec::new_builder()
                        .push(sender_script.clone())
                        .push(sudt_script.clone())
                        .build(),
                )
                .account_count(Pack::pack(&account_count))
                .kv_state(kv_state.pack())
                .return_data_hash(Pack::pack(&return_data_hash))
                .block_hashes(block_hashes)
                .build();
            VerifyTransactionWitness::new_builder()
                .l2tx(tx)
                .raw_l2block(challenged_block.raw())
                .kv_state_proof(Pack::pack(&kv_state_proof))
                .tx_proof(tx_proof)
                .block_hashes_proof(Pack::pack(&block_hashes_proof))
                .context(context)
                .build()
        };
        ckb_types::packed::WitnessArgs::new_builder()
            .lock(CKBPack::pack(&Some(witness.as_bytes())))
            .build()
    };

    let input_unlock_cell = {
        let cell = CellOutput::new_builder()
            .lock(ckb_types::packed::Script::new_unchecked(
                sudt_script.as_bytes(),
            ))
            .capacity(CKBPack::pack(&42u64))
            .build();
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let rollup_cell_data = global_state
        .clone()
        .as_builder()
        .status(Status::Running.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        input_out_point,
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .witness(CKBPack::pack(&witness.as_bytes()))
    .input(input_challenge_cell)
    .witness(CKBPack::pack(&challenge_witness.as_bytes()))
    .input(input_unlock_cell)
    .witness(Default::default())
    .cell_dep(ctx.challenge_lock_dep.clone())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .cell_dep(ctx.eoa_lock_dep.clone())
    .cell_dep(ctx.l2_sudt_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");
}
