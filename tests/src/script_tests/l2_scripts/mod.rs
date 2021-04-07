use gw_common::blake2b::new_blake2b;
use gw_common::state::State;
use gw_common::H256;
use gw_generator::{account_lock_manage::AccountLockManage, Generator};
use gw_generator::{
    error::TransactionError,
    traits::StateExt,
    types::{RollupContext, RunResult},
};
use gw_traits::{ChainStore, CodeStore};
use gw_types::packed::{RawL2Transaction, RollupConfig};
use gw_types::{
    bytes::Bytes,
    packed::{BlockInfo, LogItem},
    prelude::*,
};
use lazy_static::lazy_static;
use std::{fs, io::Read, path::PathBuf};

use crate::testing_tool::chain::build_backend_manage;

mod examples;
mod meta_contract;
mod sudt;

const EXAMPLES_DIR: &'static str = "../../godwoken-scripts/c/build/examples";
const SUM_BIN_NAME: &'static str = "sum-generator";
const ACCOUNT_OP_BIN_NAME: &'static str = "account-operation-generator";

lazy_static! {
    static ref SUM_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&EXAMPLES_DIR);
        path.push(&SUM_BIN_NAME);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    static ref SUM_PROGRAM_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&SUM_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    static ref ACCOUNT_OP_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&EXAMPLES_DIR);
        path.push(&ACCOUNT_OP_BIN_NAME);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    static ref ACCOUNT_OP_PROGRAM_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ACCOUNT_OP_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}

pub fn new_block_info(block_producer_id: u32, number: u64, timestamp: u64) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer_id(block_producer_id.pack())
        .number(number.pack())
        .timestamp(timestamp.pack())
        .build()
}

struct DummyChainStore;
impl ChainStore for DummyChainStore {
    fn get_block_hash_by_number(&self, _number: u64) -> Result<Option<H256>, gw_db::error::Error> {
        Err("dummy chain store".to_string().into())
    }
}

pub const GW_LOG_SUDT_OPERATION: u8 = 0x0;
pub const GW_LOG_POLYJUICE_SYSTEM: u8 = 0x1;
pub const GW_LOG_POLYJUICE_USER: u8 = 0x2;
pub const SUDT_OPERATION_TRANSFER: u8 = 0x0;

#[derive(Debug)]
pub struct SudtTransferLog {
    sudt_id: u32,
    from_id: u32,
    to_id: u32,
    amount: u128,
}

impl SudtTransferLog {
    fn from_log_item(item: &LogItem) -> Result<SudtTransferLog, String> {
        let sudt_id: u32 = item.account_id().unpack();
        let raw_data = item.data().raw_data();
        let log_data: &[u8] = raw_data.as_ref();
        if log_data[0] != SUDT_OPERATION_TRANSFER {
            return Err(format!("Not a sudt transfer prefix: {}", log_data[1]));
        }
        if log_data.len() != (1 + 4 + 4 + 16) {
            return Err(format!("Invalid data length: {}", log_data.len()));
        }
        let data = &log_data[1..];

        let mut u32_bytes = [0u8; 4];
        u32_bytes.copy_from_slice(&data[0..4]);
        let from_id = u32::from_le_bytes(u32_bytes.clone());

        u32_bytes.copy_from_slice(&data[4..8]);
        let to_id = u32::from_le_bytes(u32_bytes);

        let mut u128_bytes = [0u8; 16];
        u128_bytes.copy_from_slice(&data[8..24]);
        let amount = u128::from_le_bytes(u128_bytes);
        Ok(SudtTransferLog {
            sudt_id,
            from_id,
            to_id,
            amount,
        })
    }
}

pub fn check_transfer_logs(
    logs: &[LogItem],
    sudt_id: u32,
    block_producer_id: u32,
    fee: u128,
    from_id: u32,
    to_id: u32,
    amount: u128,
) {
    let sudt_fee_log = SudtTransferLog::from_log_item(&logs[0]).unwrap();
    assert_eq!(sudt_fee_log.sudt_id, sudt_id);
    assert_eq!(sudt_fee_log.from_id, from_id);
    assert_eq!(sudt_fee_log.to_id, block_producer_id);
    assert_eq!(sudt_fee_log.amount, fee);
    let sudt_transfer_log = SudtTransferLog::from_log_item(&logs[1]).unwrap();
    assert_eq!(sudt_transfer_log.sudt_id, sudt_id);
    assert_eq!(sudt_transfer_log.from_id, from_id);
    assert_eq!(sudt_transfer_log.to_id, to_id);
    assert_eq!(sudt_transfer_log.amount, amount);
}

pub fn run_contract_get_result<S: State + CodeStore>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: Bytes,
    block_info: &BlockInfo,
) -> Result<RunResult, TransactionError> {
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(to_id.pack())
        .args(args.pack())
        .build();
    let backend_manage = build_backend_manage(rollup_config);
    let account_lock_manage = AccountLockManage::default();
    let rollup_ctx = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: [42u8; 32].into(),
    };
    let generator = Generator::new(backend_manage, account_lock_manage, rollup_ctx);
    let chain_view = DummyChainStore;
    let run_result = generator.execute_transaction(&chain_view, tree, block_info, &raw_tx)?;
    tree.apply_run_result(&run_result).expect("update state");
    Ok(run_result)
}

pub fn run_contract<S: State + CodeStore>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: Bytes,
    block_info: &BlockInfo,
) -> Result<Vec<u8>, TransactionError> {
    let run_result =
        run_contract_get_result(rollup_config, tree, from_id, to_id, args, block_info)?;
    Ok(run_result.return_data)
}
