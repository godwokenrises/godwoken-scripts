#![allow(clippy::mutable_key_type)]

use crate::testing_tool::programs::ALWAYS_SUCCESS_CODE_HASH;
use anyhow::Result;
use gw_block_producer::produce_block::{
    generate_produce_block_param, produce_block, ProduceBlockParam, ProduceBlockResult,
};
use gw_chain::chain::Chain;
use gw_common::{builtins::ETH_REGISTRY_ACCOUNT_ID, registry_address::RegistryAddress, H256};
use gw_config::{
    BackendConfig, BackendSwitchConfig, BackendType, ChainConfig, GenesisConfig, MemPoolConfig,
    NodeMode,
};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_mem_pool::{
    pool::{MemPool, MemPoolCreateArgs, OutputParam},
    traits::MemPoolProvider,
};
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    core::{AllowedContractType, ScriptHashType},
    offchain::{CellInfo, DepositInfo, RollupContext},
    packed::{
        AllowedTypeHash, CellOutput, DepositLockArgs, DepositRequest, OutPoint, RollupConfig,
        Script,
    },
    prelude::*,
};
use gw_utils::local_cells::LocalCellsManager;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

// meta contract
pub const META_VALIDATOR_PATH: &str = "../c/build/meta-contract-validator";
pub const META_GENERATOR_PATH: &str = "../c/build/meta-contract-generator";
pub const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [1u8; 32];
pub const ETH_REGISTRY_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [2u8; 32];

// simple UDT
pub const SUDT_VALIDATOR_PATH: &str = "../c/build/sudt-validator";
pub const SUDT_GENERATOR_PATH: &str = "../c/build/sudt-generator";

#[derive(Debug, Default)]
pub struct DummyMemPoolProvider {
    pub fake_blocktime: Duration,
    pub deposit_cells: Vec<DepositInfo>,
}

#[async_trait::async_trait]
impl MemPoolProvider for DummyMemPoolProvider {
    async fn estimate_next_blocktime(&self) -> Result<Duration> {
        Ok(self.fake_blocktime)
    }
    async fn collect_deposit_cells(
        &self,
        _local_cells_manager: &LocalCellsManager,
    ) -> Result<Vec<DepositInfo>> {
        Ok(self.deposit_cells.clone())
    }
}

pub fn test_rollup_config() -> RollupConfig {
    let allowed_contract_type_hashes = vec![AllowedTypeHash::new(
        AllowedContractType::Meta,
        META_VALIDATOR_SCRIPT_TYPE_HASH,
    )];

    RollupConfig::new_builder()
        .allowed_contract_type_hashes(allowed_contract_type_hashes.pack())
        .build()
}

pub trait RollupConfigExtend {
    fn push_allowed_eoa_type(self, type_hash: AllowedTypeHash) -> Self;
    fn push_allowed_contract_type(self, type_hash: AllowedTypeHash) -> Self;
}

impl RollupConfigExtend for RollupConfig {
    fn push_allowed_eoa_type(self, type_hash: AllowedTypeHash) -> Self {
        let hashes_builder = self.allowed_eoa_type_hashes().as_builder();
        self.as_builder()
            .allowed_eoa_type_hashes(hashes_builder.push(type_hash).build())
            .build()
    }
    fn push_allowed_contract_type(self, type_hash: AllowedTypeHash) -> Self {
        let hashes_builder = self.allowed_contract_type_hashes().as_builder();
        self.as_builder()
            .allowed_contract_type_hashes(hashes_builder.push(type_hash).build())
            .build()
    }
}

pub struct RollupBackends<'a> {
    pub rollup_config: &'a RollupConfig,
    pub extra_backends: Option<Vec<BackendConfig>>,
}

impl<'a> RollupBackends<'a> {
    pub fn new(
        rollup_config: &'a RollupConfig,
        extra_backends: impl Into<Option<Vec<BackendConfig>>>,
    ) -> Self {
        RollupBackends {
            rollup_config,
            extra_backends: extra_backends.into(),
        }
    }
}

impl<'a> From<&'a RollupConfig> for RollupBackends<'a> {
    fn from(config: &'a RollupConfig) -> Self {
        Self::new(config, None)
    }
}

pub fn build_backend_manage(rollup_backends: RollupBackends<'_>) -> BackendManage {
    let sudt_validator_script_type_hash: [u8; 32] = { rollup_backends.rollup_config }
        .l2_sudt_validator_script_type_hash()
        .unpack();

    // Set up default backends
    let mut backends = vec![
        BackendConfig {
            validator_path: META_VALIDATOR_PATH.into(),
            generator_path: META_GENERATOR_PATH.into(),
            validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
            backend_type: BackendType::Meta,
        },
        BackendConfig {
            validator_path: SUDT_VALIDATOR_PATH.into(),
            generator_path: SUDT_GENERATOR_PATH.into(),
            validator_script_type_hash: sudt_validator_script_type_hash.into(),
            backend_type: BackendType::Sudt,
        },
    ];
    if let Some(extra_backends) = rollup_backends.extra_backends {
        backends.extend(extra_backends);
    }

    BackendManage::from_config(vec![BackendSwitchConfig {
        switch_height: 0,
        backends,
    }])
    .expect("default backend")
}

pub async fn setup_chain(rollup_type_script: Script, rollup_config: RollupConfig) -> Chain {
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage
        .register_lock_algorithm((*ALWAYS_SUCCESS_CODE_HASH).into(), Box::new(AlwaysSuccess));
    let chain = setup_chain_with_account_lock_manage(
        rollup_type_script,
        rollup_config,
        account_lock_manage,
    )
    .await;
    chain
}

pub async fn setup_chain_with_account_lock_manage(
    rollup_type_script: Script,
    rollup_config: RollupConfig,
    account_lock_manage: AccountLockManage,
) -> Chain {
    let store = Store::open_tmp().unwrap();
    let backend_manage = build_backend_manage(RollupBackends::from(&rollup_config));
    let rollup_script_hash: ckb_types::H256 = rollup_type_script.hash().into();
    let genesis_config = GenesisConfig {
        timestamp: 0,
        meta_contract_validator_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
        eth_registry_validator_type_hash: ETH_REGISTRY_VALIDATOR_SCRIPT_TYPE_HASH.into(),
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
        Default::default(),
    ));
    init_genesis(&store, &genesis_config, &[0u8; 32], Default::default()).unwrap();
    let provider = Box::new(DummyMemPoolProvider::default());
    let mem_pool_config = MemPoolConfig {
        restore_path: tempfile::TempDir::new().unwrap().path().to_path_buf(),
        ..Default::default()
    };
    let args = MemPoolCreateArgs {
        block_producer: RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, Vec::default()),
        store: store.clone(),
        generator: Arc::clone(&generator),
        provider,
        config: mem_pool_config,
        node_mode: NodeMode::FullNode,
        dynamic_config_manager: Default::default(),
        sync_server: None,
    };
    let mem_pool = MemPool::create(args).await.unwrap();
    Chain::create(
        &rollup_config,
        &rollup_type_script,
        &ChainConfig::default(),
        store,
        generator,
        Some(Arc::new(Mutex::new(mem_pool))),
    )
    .unwrap()
}

pub async fn apply_block_result(
    chain: &mut Chain,
    block_result: ProduceBlockResult,
    deposit_requests: Vec<DepositRequest>,
    deposit_asset_scripts: HashSet<Script>,
) {
    let number = block_result.block.raw().number().unpack();
    let hash = block_result.block.hash();
    let store_tx = chain.store().begin_transaction();

    let rollup_context = chain.generator().rollup_context();
    let deposit_info_vec = deposit_requests
        .into_iter()
        .map(|d| into_deposit_info_cell(&rollup_context, d).pack())
        .pack();

    chain
        .update_local(
            &store_tx,
            block_result.block,
            deposit_info_vec,
            deposit_asset_scripts,
            block_result.withdrawal_extras,
            block_result.global_state,
        )
        .unwrap();
    store_tx
        .set_block_post_finalized_custodian_capacity(
            number,
            &block_result.remaining_capacity.pack().as_reader(),
        )
        .unwrap();
    store_tx.commit().unwrap();
    let mem_pool = chain.mem_pool();
    let mut mem_pool = mem_pool.as_deref().unwrap().lock().await;
    mem_pool
        .notify_new_tip(hash.into(), &Default::default())
        .await
        .unwrap();
}

pub async fn construct_block(
    chain: &Chain,
    mem_pool: &mut MemPool,
    deposit_requests: Vec<DepositRequest>,
) -> anyhow::Result<ProduceBlockResult> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("timestamp")
        .as_millis() as u64;

    construct_block_from_timestamp(chain, mem_pool, deposit_requests, timestamp, true).await
}

pub async fn construct_block_from_timestamp(
    chain: &Chain,
    mem_pool: &mut MemPool,
    deposit_requests: Vec<DepositRequest>,
    timestamp: u64,
    refresh_mem_pool: bool,
) -> anyhow::Result<ProduceBlockResult> {
    if !refresh_mem_pool {
        assert!(
            deposit_requests.is_empty(),
            "skip refresh mem pool, but deposits isn't empty"
        )
    }
    let stake_cell_owner_lock_hash = H256::zero();
    let db = chain.store().begin_transaction();
    let generator = chain.generator();
    let rollup_config_hash = (*chain.rollup_config_hash()).into();

    let rollup_context = generator.rollup_context();
    let deposit_cells: Vec<_> = deposit_requests
        .into_iter()
        .map(|r| into_deposit_info_cell(rollup_context, r))
        .collect();

    let provider = DummyMemPoolProvider {
        deposit_cells,
        fake_blocktime: Duration::from_millis(timestamp),
    };
    mem_pool.set_provider(Box::new(provider));
    // refresh mem block
    if refresh_mem_pool {
        mem_pool.reset_mem_block(&Default::default()).await?;
    }
    let provider = DummyMemPoolProvider {
        deposit_cells: Vec::default(),
        fake_blocktime: Duration::from_millis(0),
    };
    mem_pool.set_provider(Box::new(provider));

    let (mut mem_block, post_merkle_state) = mem_pool.output_mem_block(&OutputParam::default());
    let remaining_capacity = mem_block.take_finalized_custodians_capacity();
    let block_param = generate_produce_block_param(chain.store(), mem_block, post_merkle_state)?;
    let reverted_block_root = db.get_reverted_block_smt_root().unwrap();
    let param = ProduceBlockParam {
        stake_cell_owner_lock_hash,
        rollup_config_hash,
        reverted_block_root,
        block_param,
    };
    produce_block(&db, generator, param).map(|mut r| {
        r.remaining_capacity = remaining_capacity;
        r
    })
}

pub fn into_deposit_info_cell(
    rollup_context: &RollupContext,
    request: DepositRequest,
) -> DepositInfo {
    let rollup_script_hash = rollup_context.rollup_script_hash;
    let deposit_lock_type_hash = rollup_context.rollup_config.deposit_script_type_hash();

    let lock_args = {
        let cancel_timeout = 0xc0000000000004b0u64;
        let mut buf: Vec<u8> = Vec::new();
        let deposit_args = DepositLockArgs::new_builder()
            .cancel_timeout(cancel_timeout.pack())
            .build();
        buf.extend(rollup_script_hash.as_slice());
        buf.extend(deposit_args.as_slice());
        buf
    };

    let out_point = OutPoint::new_builder()
        .tx_hash(rand::random::<[u8; 32]>().pack())
        .build();
    let lock_script = Script::new_builder()
        .code_hash(deposit_lock_type_hash)
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build();
    let output = CellOutput::new_builder()
        .lock(lock_script)
        .capacity(request.capacity())
        .build();

    let cell = CellInfo {
        out_point,
        output,
        data: request.amount().as_bytes(),
    };

    DepositInfo { cell, request }
}

pub async fn produce_empty_block(chain: &mut Chain) -> anyhow::Result<()> {
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(chain, &mut mem_pool, Default::default()).await?
    };
    let asset_scripts = HashSet::new();

    // deposit
    apply_block_result(chain, block_result, Default::default(), asset_scripts).await;
    Ok(())
}
