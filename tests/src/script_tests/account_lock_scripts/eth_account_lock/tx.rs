use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::random_out_point;
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, build_type_id_script,
    calculate_state_validator_type_id, CellContext, CellContextParam,
};
use crate::testing_tool::chain::setup_chain_with_account_lock_manage;
use crate::testing_tool::programs::STATE_VALIDATOR_CODE_HASH;
use ckb_types::{
    packed::{CellInput, CellOutput},
    prelude::{Pack as CKBPack, Unpack as CKBUnpack},
};
use gw_common::{
    h256_ext::H256Ext,
    sparse_merkle_tree::default_store::DefaultStore,
    state::{
        build_account_field_key, build_script_hash_to_account_id_key, State, GW_ACCOUNT_SCRIPT_HASH,
    },
    H256,
};
use gw_generator::account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage};
use gw_store::state_db::{StateDBTransaction, StateDBVersion};
use gw_types::prelude::*;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        Byte32, ChallengeLockArgs, ChallengeTarget, L2Transaction, RawL2Transaction, RollupAction,
        RollupActionUnion, RollupCancelChallenge, RollupConfig, Script, ScriptVec,
        VerifySignatureContext, VerifyTransactionSignatureWitness,
    },
    prelude::Unpack,
};

fn mock_account(state: &mut dyn State, id: u32, nonce: u32, script: Script) {
    let script_hash = script.hash().into();
    // nonce
    state.set_nonce(id, nonce).unwrap();
    // script hash
    state
        .update_raw(
            build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH),
            script_hash,
        )
        .unwrap();
    // script hash to id
    state
        .update_raw(
            build_script_hash_to_account_id_key(&script_hash.as_slice()),
            H256::from_u32(id),
        )
        .unwrap();
}

fn verify_tx_signature(
    eth_address: [u8; 20],
    tx: L2Transaction,
    sender_script: Script,
    receiver_script: Script,
) -> Result<u64, ckb_error::Error> {
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
    let challenge_lock_type = build_type_id_script(b"challenge_lock_type_id");
    let eth_lock_type = build_type_id_script(b"eth_lock_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let eth_lock_type_hash: [u8; 32] = eth_lock_type.calc_script_hash().unpack();

    let allowed_eoa_type_hashes: Vec<Byte32> = vec![Pack::pack(&eth_lock_type_hash)];
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .allowed_eoa_type_hashes(PackVec::pack(allowed_eoa_type_hashes))
        // .l2_sudt_validator_script_type_hash(Pack::pack(&l2_sudt_type_hash))
        // .allowed_contract_type_hashes(PackVec::pack(vec![Pack::pack(&l2_sudt_type_hash)]))
        .build();
    // setup chain
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(eth_lock_type_hash.into(), Box::new(AlwaysSuccess));
    let chain = setup_chain_with_account_lock_manage(
        rollup_type_script.clone(),
        rollup_config.clone(),
        account_lock_manage,
    );
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    // deploy scripts
    let param = CellContextParam {
        eth_lock_type: eth_lock_type.clone(),
        always_success_type: challenge_lock_type.clone(),
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
                    .target_type(ChallengeTargetType::TxSignature.into())
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
        .rollup_config_hash(Pack::pack(&rollup_config.hash()))
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
    let challenge_witness = {
        let witness = {
            let tx_proof: Bytes = {
                let mut tree: gw_common::smt::SMT<DefaultStore<H256>> = Default::default();
                for (index, tx) in challenged_block.transactions().into_iter().enumerate() {
                    tree.update(H256::from_u32(index as u32), tx.witness_hash().into())
                        .unwrap();
                }
                tree.merkle_proof(vec![H256::from_u32(challenge_target_index as u32)])
                    .unwrap()
                    .compile(vec![(
                        H256::from_u32(challenge_target_index as u32),
                        tx.witness_hash().into(),
                    )])
                    .unwrap()
                    .0
                    .into()
            };
            let db = chain.store().begin_transaction();
            let tip_block_hash = db.get_tip_block_hash().unwrap();
            let state_db = StateDBTransaction::from_version(
                &db,
                StateDBVersion::from_history_state(&db, tip_block_hash, None).unwrap(),
            )
            .unwrap();
            let mut tree = state_db.account_state_tree().unwrap();
            tree.tracker_mut().enable();
            mock_account(
                &mut tree,
                tx.raw().from_id().unpack(),
                tx.raw().nonce().unpack(),
                sender_script.clone(),
            );
            mock_account(
                &mut tree,
                tx.raw().to_id().unpack(),
                0,
                receiver_script.clone(),
            );

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
            let account_count = 2u32;
            let context = VerifySignatureContext::new_builder()
                .scripts(
                    ScriptVec::new_builder()
                        .push(sender_script.clone())
                        .push(receiver_script.clone())
                        .build(),
                )
                .account_count(Pack::pack(&account_count))
                .kv_state(kv_state.pack())
                .build();
            VerifyTransactionSignatureWitness::new_builder()
                .l2tx(tx)
                .raw_l2block(challenged_block.raw())
                .kv_state_proof(Pack::pack(&kv_state_proof))
                .tx_proof(Pack::pack(&tx_proof))
                .block_hashes_proof(Pack::pack(&block_hashes_proof))
                .context(context)
                .build()
        };
        ckb_types::packed::WitnessArgs::new_builder()
            .lock(CKBPack::pack(&Some(witness.as_bytes())))
            .build()
    };

    // Unlock cell's owner
    let (owner_lock_hash, owner_cell_input) = {
        let owner_cell = build_always_success_cell(42, None);
        let owner_lock_hash: [u8; 32] = owner_cell.lock().calc_script_hash().unpack();
        let out_point = ctx.insert_cell(owner_cell, Bytes::default());
        let owner_cell_input = CellInput::new_builder().previous_output(out_point).build();
        (owner_lock_hash, owner_cell_input)
    };
    // Eth-account-lock unlock cell
    let input_unlock_cell = {
        // hack args, inject current rollup type script hash
        let args = {
            let mut buf = Vec::new();
            buf.extend(rollup_type_script.hash().iter());
            buf.extend(eth_address.iter());
            buf
        };
        let cell = CellOutput::new_builder()
            .lock(ckb_types::packed::Script::new_unchecked(
                sender_script
                    .as_builder()
                    .args(Pack::pack(&Bytes::from(args)))
                    .code_hash(Pack::pack(&eth_lock_type_hash))
                    .hash_type(ScriptHashType::Type.into())
                    .build()
                    .as_bytes(),
            ))
            .capacity(CKBPack::pack(&42u64))
            .build();
        let data = owner_lock_hash.to_vec();
        let out_point = ctx.insert_cell(cell, Bytes::from(data));
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
    .input(owner_cell_input)
    .witness(Default::default())
    .cell_dep(ctx.challenge_lock_dep.clone())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .cell_dep(ctx.l2_sudt_dep.clone())
    .cell_dep(ctx.eth_lock_dep.clone())
    .cell_dep(ctx.secp256k1_data_dep.clone())
    .build();
    ctx.verify_tx(tx)
}

#[test]
fn test_polyjuice_call() {
    let mut polyjuice_args = vec![0u8; 52];
    polyjuice_args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
    polyjuice_args[7] = 0;
    let gas_limit: u64 = 21000;
    polyjuice_args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
    let gas_price: u128 = 20000000000;
    polyjuice_args[16..32].copy_from_slice(&gas_price.to_le_bytes());
    let value: u128 = 3000000;
    polyjuice_args[32..48].copy_from_slice(&value.to_le_bytes());
    let payload_length: u32 = 0;
    polyjuice_args[48..52].copy_from_slice(&payload_length.to_le_bytes());

    let raw_tx = RawL2Transaction::new_builder()
        .nonce(Pack::pack(&9u32))
        .to_id(Pack::pack(&1234u32))
        .args(Pack::pack(&Bytes::from(polyjuice_args)))
        .build();
    let mut signature = [0u8; 65];
    signature.copy_from_slice(&hex::decode("239ff31262bb6664d1857ea3bc5eecf3a4f74e32537c81de9fa1df2a2a48ef63115ffd8d6f5b4cc60b0fd4b02ab641106d024e49a9c0a9657c99361b39ce31ec00").expect("hex decode"));
    let tx = L2Transaction::new_builder()
        .raw(raw_tx)
        .signature(Pack::pack(&signature))
        .build();

    let rollup_type_hash = vec![0u8; 32];

    let eth_address = {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(
            &hex::decode("9d8A62f656a8d1615C1294fd71e9CFb3E4855A4F").expect("hex decode"),
        );
        buf
    };
    let mut sender_args = vec![];
    sender_args.extend(&rollup_type_hash);
    sender_args
        .extend(&hex::decode("9d8A62f656a8d1615C1294fd71e9CFb3E4855A4F").expect("hex decode"));
    let sender_script = Script::new_builder()
        .args(Pack::pack(&Bytes::from(sender_args)))
        .build();

    let mut receiver_args = vec![];
    receiver_args.extend(&rollup_type_hash);
    receiver_args.extend(&23u32.to_le_bytes());
    let receiver_script = Script::new_builder()
        .args(Pack::pack(&Bytes::from(receiver_args)))
        .build();

    verify_tx_signature(eth_address, tx, sender_script, receiver_script).expect("success");
}

#[test]
fn test_polyjuice_call_with_leading_zeros_in_to() {
    let mut polyjuice_args = vec![0u8; 52];
    polyjuice_args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
    polyjuice_args[7] = 0;
    let gas_limit: u64 = 21000;
    polyjuice_args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
    let gas_price: u128 = 20000000000;
    polyjuice_args[16..32].copy_from_slice(&gas_price.to_le_bytes());
    let value: u128 = 3000000;
    polyjuice_args[32..48].copy_from_slice(&value.to_le_bytes());
    let payload_length: u32 = 0;
    polyjuice_args[48..52].copy_from_slice(&payload_length.to_le_bytes());

    let raw_tx = RawL2Transaction::new_builder()
        .nonce(Pack::pack(&9u32))
        .to_id(Pack::pack(&1234u32))
        .args(Pack::pack(&Bytes::from(polyjuice_args)))
        .build();
    let mut signature = [0u8; 65];
    signature.copy_from_slice(&hex::decode("c49f65d9aad3b417f7d04a5e9c458b3308556bdff5a625bf65bfdadd11a18bb004bdb522991ae8648d6a1332a09576c90c93e6f9ea101bf8b5b3a7523958b50800").expect("hex decode"));
    let tx = L2Transaction::new_builder()
        .raw(raw_tx)
        .signature(Pack::pack(&signature))
        .build();

    // This rollup type hash is used, so the receiver script hash is:
    // 00002b003de527c1d67f2a2a348683ecc9598647c30884c89c5dcf6da1afbddd,
    // which contains leading zeros to ensure RLP behavior.
    let rollup_type_hash =
        hex::decode("cfdefce91f70f53167971f74bf1074b6b889be270306aabd34e67404b75dacab")
            .expect("hex decode");

    let eth_address = {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(
            &hex::decode("0000A7CE68e7328eCF2C83b103b50C68CF60Ae3a").expect("hex decode"),
        );
        buf
    };
    let mut sender_args = vec![];
    sender_args.extend(&rollup_type_hash);
    // Private key: dc88f509cab7f30ea36fd1aeb203403ce284e587bedecba73ba2fadf688acd19
    // Please do not use this private key elsewhere!
    sender_args.extend(eth_address.iter());
    let sender_script = Script::new_builder()
        .args(Pack::pack(&Bytes::from(sender_args)))
        .build();

    let mut receiver_args = vec![];
    receiver_args.extend(&rollup_type_hash);
    receiver_args.extend(&23u32.to_le_bytes());
    let receiver_script = Script::new_builder()
        .args(Pack::pack(&Bytes::from(receiver_args)))
        .build();
    verify_tx_signature(eth_address, tx, sender_script, receiver_script).expect("success");
}

#[test]
fn test_secp256k1_eth_polyjuice_create() {
    let mut polyjuice_args = vec![0u8; 69];
    polyjuice_args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
    polyjuice_args[7] = 3;
    let gas_limit: u64 = 21000;
    polyjuice_args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
    let gas_price: u128 = 20000000000;
    polyjuice_args[16..32].copy_from_slice(&gas_price.to_le_bytes());
    let value: u128 = 3000000;
    polyjuice_args[32..48].copy_from_slice(&value.to_le_bytes());
    let payload_length: u32 = 17;
    polyjuice_args[48..52].copy_from_slice(&payload_length.to_le_bytes());
    polyjuice_args[52..69].copy_from_slice(b"POLYJUICEcontract");

    let raw_tx = RawL2Transaction::new_builder()
        .nonce(Pack::pack(&9u32))
        .to_id(Pack::pack(&23u32))
        .args(Pack::pack(&Bytes::from(polyjuice_args)))
        .build();
    let mut signature = [0u8; 65];
    signature.copy_from_slice(&hex::decode("5289a4c910f143a97ce6d8ce55a970863c115bb95b404518a183ec470734ce0c10594e911d54d8894d05381fbc0f052b7397cd25217f6f102d297387a4cb15d700").expect("hex decode"));
    let tx = L2Transaction::new_builder()
        .raw(raw_tx)
        .signature(Pack::pack(&signature))
        .build();

    let rollup_type_hash = vec![0u8; 32];

    let eth_address = {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(
            &hex::decode("9d8A62f656a8d1615C1294fd71e9CFb3E4855A4F").expect("hex decode"),
        );
        buf
    };
    let mut sender_args = vec![];
    sender_args.extend(&rollup_type_hash);
    sender_args.extend(eth_address.iter());
    let sender_script = Script::new_builder()
        .args(Pack::pack(&Bytes::from(sender_args)))
        .build();

    let mut receiver_args = vec![];
    receiver_args.extend(&rollup_type_hash);
    receiver_args.extend(&23u32.to_le_bytes());
    let receiver_script = Script::new_builder()
        .args(Pack::pack(&Bytes::from(receiver_args)))
        .build();
    verify_tx_signature(eth_address, tx, sender_script, receiver_script).expect("success");
}

#[test]
fn test_secp256k1_eth_normal_call() {
    let raw_tx = RawL2Transaction::new_builder()
        .nonce(Pack::pack(&9u32))
        .to_id(Pack::pack(&1234u32))
        .build();
    let mut signature = [0u8; 65];
    signature.copy_from_slice(&hex::decode("680e9afc606f3555d75fedb41f201ade6a5f270c3a2223730e25d93e764acc6a49ee917f9e3af4727286ae4bf3ce19a5b15f71ae359cf8c0c3fabc212cccca1e00").expect("hex decode"));
    let tx = L2Transaction::new_builder()
        .raw(raw_tx)
        .signature(Pack::pack(&signature))
        .build();

    let rollup_type_hash = vec![0u8; 32];

    let eth_address = {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(
            &hex::decode("9d8A62f656a8d1615C1294fd71e9CFb3E4855A4F").expect("hex decode"),
        );
        buf
    };
    let mut sender_args = vec![];
    sender_args.extend(&rollup_type_hash);
    sender_args.extend(eth_address.iter());
    let sender_script = Script::new_builder()
        .args(Pack::pack(&Bytes::from(sender_args)))
        .build();

    let mut receiver_args = vec![];
    receiver_args.extend(&rollup_type_hash);
    receiver_args.extend(&23u32.to_le_bytes());
    let receiver_script = Script::new_builder()
        .args(Pack::pack(&Bytes::from(receiver_args)))
        .build();
    verify_tx_signature(eth_address, tx, sender_script, receiver_script).expect("success");
}
