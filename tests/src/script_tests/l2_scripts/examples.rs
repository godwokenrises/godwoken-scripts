use crate::testing_tool::chain::build_backend_manage;

use super::{
    new_block_info, DummyChainStore, ACCOUNT_OP_PROGRAM, ACCOUNT_OP_PROGRAM_CODE_HASH, SUM_PROGRAM,
    SUM_PROGRAM_CODE_HASH,
};
use gw_common::H256;
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::Backend,
    dummy_state::DummyState,
    error::TransactionError,
    traits::StateExt,
    Generator, RollupContext,
};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{RawL2Transaction, RollupConfig, Script},
    prelude::*,
};

const ERROR_ACCOUNT_NOT_FOUND: i8 = 51;

#[test]
fn test_example_sum() {
    let mut tree = DummyState::default();
    let chain_view = DummyChainStore;
    let from_id: u32 = 2;
    let init_value: u64 = 0;
    let rollup_config = RollupConfig::default();

    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUM_PROGRAM_CODE_HASH.pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");

    // run handle message
    {
        let mut backend_manage = build_backend_manage(&rollup_config);
        // NOTICE in this test we won't need SUM validator
        backend_manage.register_backend(Backend {
            validator: SUM_PROGRAM.clone(),
            generator: SUM_PROGRAM.clone(),
            validator_script_type_hash: SUM_PROGRAM_CODE_HASH.clone().into(),
        });
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage
            .register_lock_algorithm(H256::zero(), Box::new(AlwaysSuccess::default()));
        let rollup_context = RollupContext {
            rollup_config: Default::default(),
            rollup_script_hash: [42u8; 32].into(),
        };
        let generator = Generator::new(backend_manage, account_lock_manage, rollup_context);
        let mut sum_value = init_value;
        for (number, add_value) in &[(1u64, 7u64), (2u64, 16u64)] {
            let block_info = new_block_info(0, *number, 0);
            let raw_tx = RawL2Transaction::new_builder()
                .from_id(from_id.pack())
                .to_id(contract_id.pack())
                .args(Bytes::from(add_value.to_le_bytes().to_vec()).pack())
                .build();
            let run_result = generator
                .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
                .expect("construct");
            let return_value = {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&run_result.return_data);
                u64::from_le_bytes(buf)
            };
            sum_value += add_value;
            assert_eq!(return_value, sum_value);
            tree.apply_run_result(&run_result).expect("update state");
            println!("result {:?}", run_result);
        }
    }
}

#[test]
fn test_example_account_operation() {
    enum AccountOp {
        Load {
            account_id: u32,
            key: [u8; 32],
        },
        Store {
            account_id: u32,
            key: [u8; 32],
            value: [u8; 32],
        },
        LoadNonce {
            account_id: u32,
        },
        Log {
            account_id: u32,
            data: Vec<u8>,
        },
    }

    impl AccountOp {
        fn to_vec(&self) -> Vec<u8> {
            match self {
                AccountOp::Load { account_id, key } => {
                    let mut data = vec![0xF0];
                    data.extend(&account_id.to_le_bytes());
                    data.extend(key);
                    data
                }
                AccountOp::Store {
                    account_id,
                    key,
                    value,
                } => {
                    let mut data = vec![0xF1];
                    data.extend(&account_id.to_le_bytes());
                    data.extend(key);
                    data.extend(value);
                    data
                }
                AccountOp::LoadNonce { account_id } => {
                    let mut data = vec![0xF2];
                    data.extend(&account_id.to_le_bytes());
                    data
                }
                AccountOp::Log { account_id, data } => {
                    let mut args_data = vec![0xF3];
                    args_data.extend(&account_id.to_le_bytes());
                    args_data.extend(&(data.len() as u32).to_le_bytes());
                    args_data.extend(data);
                    args_data
                }
            }
        }
    }

    let mut tree = DummyState::default();
    let chain_view = DummyChainStore;
    let from_id: u32 = 2;
    let rollup_config = RollupConfig::default();

    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(ACCOUNT_OP_PROGRAM_CODE_HASH.pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");

    let mut backend_manage = build_backend_manage(&rollup_config);
    backend_manage.register_backend(Backend {
        validator: ACCOUNT_OP_PROGRAM.clone(),
        generator: ACCOUNT_OP_PROGRAM.clone(),
        validator_script_type_hash: ACCOUNT_OP_PROGRAM_CODE_HASH.clone().into(),
    });
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(H256::zero(), Box::new(AlwaysSuccess::default()));
    let rollup_context = RollupContext {
        rollup_config: Default::default(),
        rollup_script_hash: [42u8; 32].into(),
    };
    let generator = Generator::new(backend_manage, account_lock_manage, rollup_context);
    let block_info = new_block_info(0, 2, 0);

    // Load: success
    {
        let args = AccountOp::Load {
            account_id: 0,
            key: [1u8; 32],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
            .expect("result");
        assert_eq!(run_result.return_data, vec![0u8; 32]);
    }
    // Load: account not found
    {
        let args = AccountOp::Load {
            account_id: 0xff33,
            key: [1u8; 32],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_ACCOUNT_NOT_FOUND);
    }

    // Store: success
    {
        let args = AccountOp::Store {
            account_id: 0,
            key: [1u8; 32],
            value: [1u8; 32],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
            .expect("result");
        assert_eq!(run_result.return_data, Vec::<u8>::new());
    }
    // Store: account not found
    {
        let args = AccountOp::Store {
            account_id: 0xff33,
            key: [1u8; 32],
            value: [1u8; 32],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_ACCOUNT_NOT_FOUND);
    }

    // LoadNonce: success
    {
        let args = AccountOp::LoadNonce { account_id: 0 };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
            .expect("result");
        assert_eq!(run_result.return_data, 0u32.to_le_bytes().to_vec());
    }
    // LoadNonce: account not found
    {
        let args = AccountOp::LoadNonce { account_id: 0xff33 };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_ACCOUNT_NOT_FOUND);
    }

    // Log: success
    {
        let args = AccountOp::Log {
            account_id: 0,
            data: vec![3u8; 22],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
            .expect("result");
        assert_eq!(run_result.return_data, Vec::<u8>::new());
    }
    // Log: account not found
    {
        let args = AccountOp::Log {
            account_id: 0xff33,
            data: vec![3u8; 22],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_ACCOUNT_NOT_FOUND);
    }
}
