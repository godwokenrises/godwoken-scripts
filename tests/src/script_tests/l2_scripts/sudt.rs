use super::super::utils::init_env_log;
use crate::script_tests::l2_scripts::ContractExecutionEnvironment;
use crate::script_tests::utils::context::TestingContext;
use crate::testing_tool::chain::RollupConfigExtend;

use super::{check_transfer_logs, new_block_info};
use gw_common::builtins::CKB_SUDT_ACCOUNT_ID;
use gw_common::registry_address::RegistryAddress;
use gw_common::state::State;
use gw_generator::syscalls::error_codes::{
    GW_SUDT_ERROR_AMOUNT_OVERFLOW, GW_SUDT_ERROR_INSUFFICIENT_BALANCE,
};
use gw_generator::traits::StateExt;
use gw_traits::CodeStore;
use gw_types::core::AllowedContractType;
use gw_types::packed::{AllowedTypeHash, BlockInfo, Fee};
use gw_types::U256;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{RollupConfig, SUDTArgs, SUDTQuery, SUDTTransfer, Script},
    prelude::*,
};

const DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [3u8; 32];

#[test]
fn test_sudt() {
    init_env_log();
    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build()
        .push_allowed_contract_type(AllowedTypeHash::new(
            AllowedContractType::Sudt,
            DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
        ));
    let mut ctx = TestingContext::setup(&rollup_config);

    let init_a_balance = U256::from(10000u64);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 64].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 64].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    let b_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = ctx.state.get_script_hash(b_id).expect("get script hash");
    let block_producer_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([42u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = ctx
        .state
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_producer = ctx.create_eth_address(block_producer_script_hash, [42u8; 20]);
    let block_info = new_block_info(&block_producer, 1, 0);

    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);
    let b_address = ctx.create_eth_address(b_script_hash, [2u8; 20]);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");

    // init ckb for a to pay fee
    let init_ckb: U256 = 100u64.into();
    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, init_ckb)
        .expect("init balance");

    // check balance of A, B
    {
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &b_address,
            U256::zero(),
        );
    }

    // transfer from A to B
    {
        let value: U256 = 4000u128.into();
        let fee = 42u128;
        let sender_nonce = ctx.state.get_nonce(a_id).unwrap();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(b_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let mut exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        let new_sender_nonce = ctx.state.get_nonce(a_id).unwrap();
        assert_eq!(sender_nonce + 1, new_sender_nonce, "nonce increased");
        assert!(run_result.return_data.is_empty());
        assert_eq!(run_result.write.logs.len(), 2);
        check_transfer_logs(
            &run_result.write.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &b_address,
            value,
        );

        {
            // check sender's sudt
            check_balance(
                &rollup_config,
                &mut ctx.state,
                &block_info,
                a_id,
                sudt_id,
                &a_address,
                init_a_balance - value,
            );

            // check sender's ckb
            check_balance(
                &rollup_config,
                &mut ctx.state,
                &block_info,
                a_id,
                CKB_SUDT_ACCOUNT_ID,
                &a_address,
                init_ckb - fee,
            );

            // check receiver's sudt
            check_balance(
                &rollup_config,
                &mut ctx.state,
                &block_info,
                a_id,
                sudt_id,
                &b_address,
                value,
            );

            // check receiver's ckb
            check_balance(
                &rollup_config,
                &mut ctx.state,
                &block_info,
                a_id,
                CKB_SUDT_ACCOUNT_ID,
                &b_address,
                U256::zero(),
            );

            // check producers's sudt
            check_balance(
                &rollup_config,
                &mut ctx.state,
                &block_info,
                a_id,
                sudt_id,
                &block_producer,
                U256::zero(),
            );

            // check producers's ckb
            check_balance(
                &rollup_config,
                &mut ctx.state,
                &block_info,
                a_id,
                CKB_SUDT_ACCOUNT_ID,
                &block_producer,
                fee.into(),
            );
        }
    }
}

#[test]
fn test_insufficient_balance() {
    init_env_log();
    let init_a_balance = U256::from(10000);

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build()
        .push_allowed_contract_type(AllowedTypeHash::new(
            AllowedContractType::Sudt,
            DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
        ));
    let mut ctx = TestingContext::setup(&rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");

    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    let b_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = ctx.state.get_script_hash(b_id).expect("get script hash");

    let block_info = new_block_info(&Default::default(), 10, 0);

    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);
    let b_address = ctx.create_eth_address(b_script_hash, [2u8; 20]);
    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");

    // transfer from A to B
    {
        let value: U256 = 10001u128.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(b_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .unhandle_execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        assert_eq!(run_result.exit_code, GW_SUDT_ERROR_INSUFFICIENT_BALANCE);
    }
}

#[test]
fn test_transfer_to_non_exist_account() {
    let init_a_balance = U256::from(10000);

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build()
        .push_allowed_contract_type(AllowedTypeHash::new(
            AllowedContractType::Sudt,
            DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
        ));
    let mut ctx = TestingContext::setup(&rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);
    let b_address = RegistryAddress::new(a_address.registry_id, [0x33u8; 20].to_vec());

    let block_info = new_block_info(&Default::default(), 10, 0);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");

    // transfer from A to B
    {
        let value: U256 = 1000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(b_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let mut exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let _run_result = exec_env
            .execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
    }
}

#[test]
fn test_transfer_to_self() {
    let init_a_balance = U256::from(10000u64);
    let init_ckb: U256 = 100u64.into();

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build()
        .push_allowed_contract_type(AllowedTypeHash::new(
            AllowedContractType::Sudt,
            DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
        ));
    let mut ctx = TestingContext::setup(&rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);

    let block_producer_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = ctx
        .state
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_producer = ctx.create_eth_address(block_producer_script_hash, [42u8; 20]);
    let block_producer_balance = U256::zero();
    let block_info = new_block_info(&block_producer, 10, 0);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");

    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, init_ckb)
        .expect("init balance");

    // transfer from A to A, zero value
    {
        let value = U256::zero();
        let fee = 0u128;
        let sender_nonce = ctx.state.get_nonce(a_id).unwrap();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let mut exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        let new_sender_nonce = ctx.state.get_nonce(a_id).unwrap();
        assert_eq!(sender_nonce + 1, new_sender_nonce, "nonce increased");
        assert_eq!(run_result.write.logs.len(), 2);
        check_transfer_logs(
            &run_result.write.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &a_address,
            value,
        );
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
    }

    // transfer from A to A, normal value
    let fee = 20u128;
    {
        let value: U256 = 1000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let mut exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        assert_eq!(run_result.write.logs.len(), 2);
        check_transfer_logs(
            &run_result.write.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &a_address,
            value,
        );

        // sender's sudt balance
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        // sender's ckb balance
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb - fee,
        );

        // block producer's balance
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );

        // block producer's balance
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            fee.into(),
        );
    }

    // transfer from A to A, insufficient balance
    {
        let value: U256 = 100000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .unhandle_execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        assert_eq!(run_result.exit_code, GW_SUDT_ERROR_INSUFFICIENT_BALANCE);
        // sender sudt
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        // sender's ckb
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb - fee,
        );
        // block producer ckb
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            fee.into(),
        );
    }
}

#[test]
fn test_transfer_to_self_overflow() {
    let init_a_balance: U256 = U256::MAX - U256::one();
    let init_ckb = U256::from(100u64);

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build()
        .push_allowed_contract_type(AllowedTypeHash::new(
            AllowedContractType::Sudt,
            DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
        ));
    let mut ctx = TestingContext::setup(&rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);

    let block_producer_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = ctx
        .state
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_producer = ctx.create_eth_address(block_producer_script_hash, [42u8; 20]);
    let block_producer_balance = U256::zero();
    let block_info = new_block_info(&block_producer, 10, 0);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");
    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, init_ckb)
        .expect("init balance");

    // transfer from A to A, zero value
    {
        let value = U256::zero();
        let fee = 0u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let mut exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        assert_eq!(run_result.write.logs.len(), 2);
        check_transfer_logs(
            &run_result.write.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &a_address,
            value,
        );

        // sender's sudt
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        // sender's ckb
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb,
        );
        // block producer's sudt
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        // block producer's ckb
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            U256::zero(),
        );
    }

    // transfer from A to A, 1 value
    {
        let value = U256::one();
        let fee = 0u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .amount(fee.pack())
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let mut exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        assert_eq!(run_result.write.logs.len(), 2);
        check_transfer_logs(
            &run_result.write.logs,
            sudt_id,
            &block_producer,
            fee,
            &a_address,
            &a_address,
            value,
        );
        // sudt
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        // ckb
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb,
        );
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            U256::zero(),
        );
    }

    // transfer from A to A, overflow balance
    {
        let value: U256 = 100000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let mut exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        assert_eq!(run_result.write.logs.len(), 2);
        check_transfer_logs(
            &run_result.write.logs,
            sudt_id,
            &block_producer,
            0,
            &a_address,
            &a_address,
            value,
        );
        // sudt
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        // ckb
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb,
        );
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            U256::zero(),
        );
    }

    // transfer from A to A with a large value
    {
        let value: U256 = U256::MAX - U256::one();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(a_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let mut exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        assert_eq!(run_result.write.logs.len(), 2);
        check_transfer_logs(
            &run_result.write.logs,
            sudt_id,
            &block_producer,
            0,
            &a_address,
            &a_address,
            value,
        );
        //sudt
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &block_producer,
            block_producer_balance,
        );
        //ckb
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_ckb,
        );
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &block_producer,
            U256::zero(),
        );
    }
}

#[test]
#[ignore = "total supply overflow"]
fn test_transfer_overflow() {
    let init_a_balance = U256::from(10000u64);
    let init_b_balance: U256 = U256::MAX - init_a_balance;
    let init_a_ckb = U256::from(100u64);

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();
    let mut ctx = TestingContext::setup(&rollup_config);

    // init accounts
    let _meta = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let sudt_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = ctx.state.get_script_hash(a_id).expect("get script hash");
    let a_address = ctx.create_eth_address(a_script_hash, [1u8; 20]);
    let b_id = ctx
        .state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = ctx.state.get_script_hash(b_id).expect("get script hash");
    let b_address = ctx.create_eth_address(b_script_hash, [2u8; 20]);

    let block_info = new_block_info(&Default::default(), 10, 0);

    // init balance for a
    ctx.state
        .mint_sudt(sudt_id, &a_address, init_a_balance)
        .expect("init balance");
    ctx.state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_address, init_a_ckb)
        .expect("init balance");
    ctx.state
        .mint_sudt(sudt_id, &b_address, init_b_balance)
        .expect("init balance");

    // transfer from A to B overflow
    {
        let value: U256 = 10000u64.into();
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Bytes::from(b_address.to_bytes()).pack())
                    .amount(value.pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(a_address.registry_id.pack())
                            .build(),
                    )
                    .build(),
            )
            .build();
        let exec_env = ContractExecutionEnvironment::new(&rollup_config, &mut ctx.state);
        let run_result = exec_env
            .unhandle_execute(a_id, sudt_id, args.as_bytes(), &block_info)
            .expect("execute");
        assert_eq!(run_result.exit_code, GW_SUDT_ERROR_AMOUNT_OVERFLOW);

        // check balance
        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            CKB_SUDT_ACCOUNT_ID,
            &a_address,
            init_a_ckb,
        );

        check_balance(
            &rollup_config,
            &mut ctx.state,
            &block_info,
            a_id,
            sudt_id,
            &b_address,
            init_b_balance,
        );
    }
}

fn check_balance<S: State + CodeStore>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    block_info: &BlockInfo,
    sender_id: u32,
    sudt_id: u32,
    address: &RegistryAddress,
    expected_balance: U256,
) {
    // check balance
    let args = SUDTArgs::new_builder()
        .set(
            SUDTQuery::new_builder()
                .address(Bytes::from(address.to_bytes()).pack())
                .build(),
        )
        .build();
    let mut exec_env = ContractExecutionEnvironment::new(rollup_config, tree);
    let run_result = exec_env
        .execute(sender_id, sudt_id, args.as_bytes(), block_info)
        .expect("execute");
    let return_data = run_result.return_data.to_vec();
    let balance = {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&return_data);
        U256::from_little_endian(&buf)
    };
    assert_eq!(balance, expected_balance);
}
