use crate::testing_tool::programs::ALWAYS_SUCCESS_CODE_HASH;
use gw_block_producer::{
    produce_block::{produce_block, ProduceBlockParam, ProduceBlockResult},
    withdrawal::AvailableCustodians,
};
use gw_chain::chain::{Chain, L1Action, L1ActionContext, SyncEvent, SyncParam};
use gw_config::{BackendConfig, GenesisConfig};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    types::RollupContext,
    Generator,
};
use gw_mem_pool::pool::MemPool;
use gw_store::Store;
use gw_types::{
    packed::{
        CellOutput, DepositRequest, L2BlockCommittedInfo, RawTransaction, RollupAction,
        RollupActionUnion, RollupConfig, RollupSubmitBlock, Script, Transaction, WitnessArgs,
    },
    prelude::*,
};
use parking_lot::Mutex;
use std::sync::Arc;

// meta contract
pub const META_VALIDATOR_PATH: &str = "../c/build/meta-contract-validator";
pub const META_GENERATOR_PATH: &str = "../c/build/meta-contract-generator";
pub const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [1u8; 32];

// simple UDT
pub const SUDT_VALIDATOR_PATH: &str = "../c/build/sudt-validator";
pub const SUDT_GENERATOR_PATH: &str = "../c/build/sudt-generator";

pub fn build_backend_manage(rollup_config: &RollupConfig) -> BackendManage {
    let sudt_validator_script_type_hash: [u8; 32] =
        rollup_config.l2_sudt_validator_script_type_hash().unpack();
    let configs = vec![
        BackendConfig {
            validator_path: META_VALIDATOR_PATH.into(),
            generator_path: META_GENERATOR_PATH.into(),
            validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
        },
        BackendConfig {
            validator_path: SUDT_VALIDATOR_PATH.into(),
            generator_path: SUDT_GENERATOR_PATH.into(),
            validator_script_type_hash: sudt_validator_script_type_hash.into(),
        },
    ];
    BackendManage::from_config(configs).expect("default backend")
}

pub fn setup_chain(rollup_type_script: Script, rollup_config: RollupConfig) -> Chain {
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(
        ALWAYS_SUCCESS_CODE_HASH.clone().into(),
        Box::new(AlwaysSuccess),
    );
    setup_chain_with_account_lock_manage(rollup_type_script, rollup_config, account_lock_manage)
}

pub fn setup_chain_with_account_lock_manage(
    rollup_type_script: Script,
    rollup_config: RollupConfig,
    account_lock_manage: AccountLockManage,
) -> Chain {
    let store = Store::open_tmp().unwrap();
    let genesis_l2block_committed_info = L2BlockCommittedInfo::default();
    let backend_manage = build_backend_manage(&rollup_config);
    let rollup_script_hash: ckb_types::H256 = rollup_type_script.hash().into();
    let genesis_config = GenesisConfig {
        timestamp: 0,
        meta_contract_validator_type_hash: Default::default(),
        rollup_type_hash: rollup_script_hash.clone().0.into(),
        rollup_config: rollup_config.clone().into(),
        secp_data_dep: Default::default(),
    };
    let rollup_context = RollupContext {
        rollup_script_hash: rollup_script_hash.0.into(),
        rollup_config: rollup_config.clone(),
    };
    let generator = Arc::new(Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
    ));
    init_genesis(
        &store,
        &genesis_config,
        genesis_l2block_committed_info,
        Default::default(),
    )
    .unwrap();
    let mem_pool = MemPool::create(store.clone(), Arc::clone(&generator)).unwrap();
    Chain::create(
        &rollup_config,
        &rollup_type_script,
        store,
        generator,
        Arc::new(Mutex::new(mem_pool)),
    )
    .unwrap()
}

pub fn build_sync_tx(
    rollup_cell: CellOutput,
    produce_block_result: ProduceBlockResult,
) -> Transaction {
    let ProduceBlockResult {
        block,
        global_state,
        unused_transactions,
        unused_withdrawal_requests,
    } = produce_block_result;
    assert!(unused_transactions.is_empty());
    assert!(unused_withdrawal_requests.is_empty());
    let action = RollupAction::new_builder()
        .set(RollupActionUnion::RollupSubmitBlock(
            RollupSubmitBlock::new_builder().block(block).build(),
        ))
        .build();
    let witness = WitnessArgs::new_builder()
        .output_type(Pack::<_>::pack(&Some(action.as_bytes())))
        .build();
    let raw = RawTransaction::new_builder()
        .outputs(vec![rollup_cell].pack())
        .outputs_data(vec![global_state.as_bytes()].pack())
        .build();
    Transaction::new_builder()
        .raw(raw)
        .witnesses(vec![witness.as_bytes()].pack())
        .build()
}

pub fn apply_block_result(
    chain: &mut Chain,
    rollup_cell: CellOutput,
    block_result: ProduceBlockResult,
    deposit_requests: Vec<DepositRequest>,
) {
    let transaction = build_sync_tx(rollup_cell, block_result);
    let l2block_committed_info = L2BlockCommittedInfo::default();

    let update = L1Action {
        context: L1ActionContext::SubmitTxs { deposit_requests },
        transaction,
        l2block_committed_info,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
    };
    let event = chain.sync(param).unwrap();
    assert_eq!(event, SyncEvent::Success);
}

pub fn construct_block(
    chain: &Chain,
    mem_pool: &MemPool,
    deposit_requests: Vec<DepositRequest>,
) -> anyhow::Result<ProduceBlockResult> {
    let block_producer_id = 0u32;
    let timestamp = 0;
    let stake_cell_owner_lock_hash = gw_common::H256::zero();
    let max_withdrawal_capacity = std::u128::MAX;
    let db = chain.store().begin_transaction();
    let generator = chain.generator();
    let parent_block = chain.store().get_tip_block().unwrap();
    let rollup_config_hash = chain.rollup_config_hash().clone().into();
    let mut txs = Vec::new();
    let mut available_custodians = AvailableCustodians::default();
    // initialize with some withdraw-able capacity
    available_custodians.capacity += 10000 * (100000000);
    let mut withdrawal_requests = Vec::new();
    for (_, entry) in mem_pool.pending() {
        // notice we either choice txs or withdrawals from an entry to avoid nonce conflict
        if !entry.txs.is_empty() {
            txs.extend(entry.txs.iter().cloned());
        } else if !entry.withdrawals.is_empty() {
            for w in &entry.withdrawals {
                let raw = w.raw();
                let capacity: u64 = raw.capacity().unpack();
                available_custodians.capacity += capacity as u128;
                let entry = available_custodians
                    .sudt
                    .entry(raw.sudt_script_hash().unpack());
                let v = entry.or_default();
                v.0 += raw.amount().unpack();
            }
            withdrawal_requests.extend(entry.withdrawals.iter().cloned());
        }
    }

    let param = ProduceBlockParam {
        db,
        generator,
        block_producer_id,
        stake_cell_owner_lock_hash,
        timestamp,
        txs,
        deposit_requests,
        withdrawal_requests,
        parent_block: &parent_block,
        rollup_config_hash: &rollup_config_hash,
        max_withdrawal_capacity,
        available_custodians,
    };
    produce_block(param)
}
