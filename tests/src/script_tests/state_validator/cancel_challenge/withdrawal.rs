#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;

use crate::script_tests::state_validator::cancel_challenge::build_merkle_proof;
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::random_out_point;
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, build_type_id_script,
    calculate_state_validator_type_id, CellContext, CellContextParam,
};
use crate::testing_tool::chain::{
    apply_block_result, construct_block, setup_chain_with_account_lock_manage,
};
use crate::testing_tool::programs::STATE_VALIDATOR_CODE_HASH;
use ckb_types::{
    packed::{CellInput, CellOutput},
    prelude::{Pack as CKBPack, Unpack as CKBUnpack},
};
use gw_common::registry_address::RegistryAddress;
use gw_common::H256;
use gw_generator::account_lock_manage::{
    eip712::{
        traits::EIP712Encode,
        types::{EIP712Domain, Withdrawal},
    },
    {always_success::AlwaysSuccess, AccountLockManage},
};
use gw_store::state::state_db::StateContext;
use gw_types::core::AllowedEoaType;
use gw_types::core::SigningType;
use gw_types::packed::AllowedTypeHash;
use gw_types::packed::CCWithdrawalWitness;
use gw_types::packed::WithdrawalRequestExtra;
use gw_types::prelude::*;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        ChallengeLockArgs, ChallengeTarget, DepositRequest, RawWithdrawalRequest, RollupAction,
        RollupActionUnion, RollupCancelChallenge, RollupConfig, Script, WithdrawalRequest,
    },
};

#[tokio::test]
async fn test_cancel_withdrawal() {
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
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let eoa_lock_type_hash: [u8; 32] = eoa_lock_type.calc_script_hash().unpack();
    let allowed_eoa_type_hashes: Vec<AllowedTypeHash> = vec![AllowedTypeHash::new(
        AllowedEoaType::Eth,
        eoa_lock_type_hash,
    )];
    let finality_blocks = 10;
    let eth_registry_id = gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .allowed_eoa_type_hashes(PackVec::pack(allowed_eoa_type_hashes))
        .finality_blocks(Pack::pack(&finality_blocks))
        .build();
    // setup chain
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(eoa_lock_type_hash.into(), Box::new(AlwaysSuccess));
    let mut chain = setup_chain_with_account_lock_manage(
        rollup_type_script.clone(),
        rollup_config.clone(),
        account_lock_manage,
    )
    .await;
    chain.complete_initial_syncing().await.unwrap();
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    // produce a block so we can challenge it
    let rollup_script_hash = rollup_type_script.hash();

    let withdrawal_extra;
    let sender_script = {
        // deposit two account
        let mut sender_args = rollup_script_hash.to_vec();
        sender_args.extend_from_slice(&[1u8; 20]);
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&eoa_lock_type_hash.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(sender_args)))
            .build();
        let mut receiver_args = rollup_script_hash.to_vec();
        receiver_args.extend_from_slice(&[2u8; 20]);
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&eoa_lock_type_hash.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(receiver_args)))
            .build();
        let deposit_requests = vec![
            DepositRequest::new_builder()
                .capacity(Pack::pack(&450_00000000u64))
                .script(sender_script.clone())
                .registry_id(Pack::pack(&eth_registry_id))
                .build(),
            DepositRequest::new_builder()
                .capacity(Pack::pack(&550_00000000u64))
                .script(receiver_script)
                .registry_id(Pack::pack(&eth_registry_id))
                .build(),
        ];
        let produce_block_result = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = mem_pool.lock().await;
            construct_block(&chain, &mut mem_pool, deposit_requests.clone())
                .await
                .unwrap()
        };
        let rollup_cell = gw_types::packed::CellOutput::new_unchecked(rollup_cell.as_bytes());
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            rollup_cell.clone(),
            produce_block_result,
            deposit_requests,
            asset_scripts,
        )
        .await;
        {
            use gw_common::state::State;
            let db = chain.store().begin_transaction();
            let tree = db.state_tree(StateContext::ReadOnly).unwrap();
            let value = tree
                .get_sudt_balance(1, &RegistryAddress::new(eth_registry_id, vec![1u8; 20]))
                .unwrap();
            dbg!("balance", value);

            let registry_address = tree
                .get_registry_address_by_script_hash(eth_registry_id, &sender_script.hash().into())
                .unwrap()
                .unwrap();
            dbg!(registry_address.address.len());
        }
        let withdrawal_capacity = 400_00000000u64;
        withdrawal_extra = {
            let owner_lock = Script::default();
            WithdrawalRequestExtra::new_builder()
                .request(
                    WithdrawalRequest::new_builder()
                        .raw(
                            RawWithdrawalRequest::new_builder()
                                .nonce(Pack::pack(&0u32))
                                .capacity(Pack::pack(&withdrawal_capacity))
                                .account_script_hash(Pack::pack(&sender_script.hash()))
                                .owner_lock_hash(Pack::pack(&owner_lock.hash()))
                                .registry_id(Pack::pack(&eth_registry_id))
                                .build(),
                        )
                        .build(),
                )
                .owner_lock(owner_lock)
                .build()
        };
        let produce_block_result = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = mem_pool.lock().await;
            mem_pool
                .push_withdrawal_request(withdrawal_extra.clone())
                .await
                .unwrap();
            construct_block(&chain, &mut mem_pool, Vec::default())
                .await
                .unwrap()
        };
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            rollup_cell,
            produce_block_result,
            vec![],
            asset_scripts,
        )
        .await;
        sender_script
    };
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        challenge_lock_type,
        eoa_lock_type,
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
                    .target_type(ChallengeTargetType::Withdrawal.into())
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
    let withdrawal = challenged_block
        .withdrawals()
        .get(challenge_target_index as usize)
        .unwrap();
    let challenge_witness = {
        let witness = {
            let leaves: Vec<H256> = challenged_block
                .withdrawals()
                .into_iter()
                .enumerate()
                .map(|(_, withdrawal)| withdrawal.witness_hash().into())
                .collect();
            let proof = build_merkle_proof(&leaves, &[challenge_target_index]);
            // we do not actually execute the signature verification in this test
            CCWithdrawalWitness::new_builder()
                .raw_l2block(challenged_block.raw())
                .withdrawal(withdrawal.clone())
                .sender(sender_script.clone())
                .owner_lock(withdrawal_extra.owner_lock())
                .withdrawal_proof(proof)
                .build()
        };
        ckb_types::packed::WitnessArgs::new_builder()
            .lock(CKBPack::pack(&Some(witness.as_bytes())))
            .build()
    };
    let input_unlock_cell = {
        let cell = CellOutput::new_builder()
            .lock(ckb_types::packed::Script::new_unchecked(
                sender_script.as_bytes(),
            ))
            .capacity(CKBPack::pack(&42u64))
            .build();
        let owner_lock_hash = vec![42u8; 32];
        let message = {
            let withdrawal = Withdrawal::from_withdrawal_request(
                withdrawal.raw(),
                withdrawal_extra.owner_lock(),
            )
            .unwrap();
            let domain = EIP712Domain {
                name: "Godwoken".to_string(),
                version: "1".to_string(),
                chain_id: withdrawal_extra.raw().chain_id().unpack(),
                verifying_contract: None,
                salt: None,
            };
            withdrawal.eip712_message(domain.hash_struct())
        };
        let mut buf = owner_lock_hash;
        buf.push(SigningType::Raw.into());
        buf.extend_from_slice(&message);
        let out_point = ctx.insert_cell(cell, Bytes::from(buf));
        CellInput::new_builder().previous_output(out_point).build()
    };
    let rollup_cell_data = global_state
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
    .build();
    ctx.verify_tx(tx).expect("return success");
}
