use crate::testing_tool::chain::build_backend_manage;

use super::{
    new_block_info, DummyChainStore, SudtLog, SudtLogType, ACCOUNT_OP_PROGRAM,
    ACCOUNT_OP_PROGRAM_CODE_HASH, GW_LOG_SUDT_TRANSFER, RECOVER_PROGRAM, RECOVER_PROGRAM_CODE_HASH,
    SUM_PROGRAM, SUM_PROGRAM_CODE_HASH,
};
use gw_common::{h256_ext::H256Ext, H256};
use gw_config::BackendType;
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, secp256k1::Secp256k1, AccountLockManage},
    backend_manage::Backend,
    constants::L2TX_MAX_CYCLES,
    dummy_state::DummyState,
    error::TransactionError,
    syscalls::error_codes::{GW_ERROR_ACCOUNT_NOT_FOUND, GW_ERROR_RECOVER, GW_FATAL_UNKNOWN_ARGS},
    traits::StateExt,
    Generator,
};
use gw_types::offchain::RollupContext;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{RawL2Transaction, RollupConfig, Script},
    prelude::*,
};

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
            backend_type: BackendType::Unknown,
        });
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage
            .register_lock_algorithm(H256::zero(), Box::new(AlwaysSuccess::default()));
        let rollup_context = RollupContext {
            rollup_config: Default::default(),
            rollup_script_hash: [42u8; 32].into(),
        };
        let generator = Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
            Default::default(),
        );
        let mut sum_value = init_value;
        for (number, add_value) in &[(1u64, 7u64), (2u64, 16u64)] {
            let block_info = new_block_info(0, *number, 0);
            let raw_tx = RawL2Transaction::new_builder()
                .from_id(from_id.pack())
                .to_id(contract_id.pack())
                .args(Bytes::from(add_value.to_le_bytes().to_vec()).pack())
                .build();
            let run_result = generator
                .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
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

pub enum AccountOp {
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
        service_flag: u8,
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
            AccountOp::Log {
                account_id,
                service_flag,
                data,
            } => {
                let mut args_data = vec![0xF3];
                args_data.extend(&account_id.to_le_bytes());
                args_data.push(*service_flag);
                args_data.extend(&(data.len() as u32).to_le_bytes());
                args_data.extend(data);
                args_data
            }
        }
    }
}

#[test]
fn test_example_account_operation() {
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
        backend_type: BackendType::Unknown,
    });
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(H256::zero(), Box::new(AlwaysSuccess::default()));
    let rollup_context = RollupContext {
        rollup_config: Default::default(),
        rollup_script_hash: [42u8; 32].into(),
    };
    let generator = Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
        Default::default(),
    );
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
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
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
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_ERROR_ACCOUNT_NOT_FOUND as i8);
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
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
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
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_ERROR_ACCOUNT_NOT_FOUND as i8);
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
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
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
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_ERROR_ACCOUNT_NOT_FOUND as i8);
    }

    // Log: success
    {
        let account_id = 0;
        let from_script_hash = [0x33u8; 32];
        let to_script_hash = [0x44u8; 32];
        let amount: u128 = 101;
        let mut data = vec![0u8; 1 + 20 + 20 + 16];
        data[0] = 20;
        data[1..21].copy_from_slice(&from_script_hash[0..20]);
        data[21..41].copy_from_slice(&to_script_hash[0..20]);
        data[41..41 + 16].copy_from_slice(&amount.to_le_bytes()[..]);
        let args = AccountOp::Log {
            service_flag: GW_LOG_SUDT_TRANSFER,
            account_id,
            data,
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
            .expect("result");
        let log = SudtLog::from_log_item(&run_result.logs[0]).unwrap();
        assert_eq!(log.sudt_id, account_id);
        assert_eq!(log.from_addr, from_script_hash[0..20]);
        assert_eq!(log.to_addr, to_script_hash[0..20]);
        assert_eq!(log.amount, amount);
        assert_eq!(log.log_type, SudtLogType::Transfer);
        assert_eq!(run_result.return_data, Vec::<u8>::new());
    }
    // Log: account not found
    {
        let args = AccountOp::Log {
            account_id: 0xff33,
            service_flag: GW_LOG_SUDT_TRANSFER,
            data: vec![3u8; 22],
        };
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args.to_vec()).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, GW_ERROR_ACCOUNT_NOT_FOUND as i8);
    }
}

#[test]
fn test_example_recover_account() {
    let mut tree = DummyState::default();
    let chain_view = DummyChainStore;
    let from_id: u32 = 2;
    let rollup_config = RollupConfig::default();

    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(RECOVER_PROGRAM_CODE_HASH.pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");

    let mut backend_manage = build_backend_manage(&rollup_config);
    backend_manage.register_backend(Backend {
        validator: RECOVER_PROGRAM.clone(),
        generator: RECOVER_PROGRAM.clone(),
        validator_script_type_hash: RECOVER_PROGRAM_CODE_HASH.clone().into(),
        backend_type: BackendType::Unknown,
    });
    let mut account_lock_manage = AccountLockManage::default();
    let secp256k1_code_hash = H256::from_u32(11);
    account_lock_manage
        .register_lock_algorithm(secp256k1_code_hash, Box::new(Secp256k1::default()));
    let rollup_script_hash: H256 = [42u8; 32].into();
    let rollup_context = RollupContext {
        rollup_config: Default::default(),
        rollup_script_hash,
    };
    let generator = Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
        Default::default(),
    );
    let block_info = new_block_info(0, 2, 0);

    let lock_args_hex = "404f90829ec0e5821aeba9bce7d5e841ce9f7fa5";
    let message_hex = "1cdeae55a5768fe14b628001c6247ae84c70310a7ddcfdc73ac68494251e46ec";
    let signature_hex = "28aa0c394487edf2211f445c47fb5f4fb5e3023920f62124d309f5bdf70d95045a934f278cec717300a5417313d1cdc390e761e37c0964b940c0a6f07b7361ed01";

    // success
    {
        let mut args = vec![0u8; 32 + 1 + 65 + 32];
        args[0..32].copy_from_slice(&hex::decode(message_hex).unwrap());
        args[32] = 65;
        args[33..33 + 65].copy_from_slice(&hex::decode(signature_hex).unwrap());
        args[33 + 65..33 + 65 + 32].copy_from_slice(secp256k1_code_hash.as_slice());
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let run_result = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
            .expect("result");
        let mut script_args = vec![0u8; 32 + 20];
        script_args[0..32].copy_from_slice(rollup_script_hash.as_slice());
        script_args[32..32 + 20].copy_from_slice(&hex::decode(lock_args_hex).unwrap());
        let script = Script::new_builder()
            .code_hash(secp256k1_code_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from(script_args).pack())
            .build();
        assert_eq!(&run_result.return_data, script.as_slice());
    }

    // Error signature
    {
        let mut args = vec![0u8; 32 + 1 + 65 + 32];
        let error_signature_hex = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
        args[0..32].copy_from_slice(&hex::decode(message_hex).unwrap());
        args[32] = 65;
        args[33..33 + 65].copy_from_slice(&hex::decode(error_signature_hex).unwrap());
        args[33 + 65..33 + 65 + 32].copy_from_slice(secp256k1_code_hash.as_slice());
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        println!("err_code: {}", err_code);
        assert_eq!(err_code, GW_ERROR_RECOVER as i8);
    }

    // Wrong code hash
    {
        let mut args = vec![0u8; 32 + 1 + 65 + 32];
        let wrong_code_hash = H256::from_u32(22);
        args[0..32].copy_from_slice(&hex::decode(message_hex).unwrap());
        args[32] = 65;
        args[33..33 + 65].copy_from_slice(&hex::decode(signature_hex).unwrap());
        args[33 + 65..33 + 65 + 32].copy_from_slice(wrong_code_hash.as_slice());
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let err = generator
            .execute_transaction(&chain_view, &tree, &block_info, &raw_tx, L2TX_MAX_CYCLES)
            .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        println!("err_code: {}", err_code);
        assert_eq!(err_code, GW_FATAL_UNKNOWN_ARGS);
    }
}
