use super::{check_transfer_logs, new_block_info, run_contract, run_contract_get_result};
use gw_common::state::{to_short_address, State};
use gw_generator::dummy_state::DummyState;
use gw_generator::{error::TransactionError, traits::StateExt};
use gw_traits::CodeStore;
use gw_types::packed::BlockInfo;
use gw_types::{
    core::ScriptHashType,
    packed::{RollupConfig, SUDTArgs, SUDTQuery, SUDTTransfer, Script},
    prelude::*,
};

const ERROR_INSUFFICIENT_BALANCE: i8 = 12i8;
const ERROR_AMOUNT_OVERFLOW: i8 = 13i8;
const DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [3u8; 32];

#[test]
fn test_sudt() {
    let mut tree = DummyState::default();

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();

    let init_a_balance: u128 = 10000;

    // init accounts
    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 64].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = tree.get_script_hash(a_id).expect("get script hash");
    let b_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = tree.get_script_hash(b_id).expect("get script hash");
    let block_producer_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = tree
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_info = new_block_info(block_producer_id, 1, 0);

    // init balance for a
    tree.mint_sudt(sudt_id, to_short_address(&a_script_hash), init_a_balance)
        .expect("init balance");

    let a_address = to_short_address(&a_script_hash).to_vec();
    let b_address = to_short_address(&b_script_hash).to_vec();
    let block_producer_address = to_short_address(&block_producer_script_hash).to_vec();
    // check balance of A, B
    {
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &b_address,
            0,
        );
    }

    // transfer from A to B
    {
        let value = 4000u128;
        let fee = 42u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(b_address.pack())
                    .amount(value.pack())
                    .fee(fee.pack())
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("execute");
        assert!(run_result.return_data.is_empty());
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            block_producer_script_hash,
            fee,
            a_script_hash,
            b_script_hash,
            value,
        );

        {
            check_balance(
                &rollup_config,
                &mut tree,
                &block_info,
                a_id,
                sudt_id,
                &a_address,
                init_a_balance - value - fee,
            );

            check_balance(
                &rollup_config,
                &mut tree,
                &block_info,
                a_id,
                sudt_id,
                &b_address,
                value,
            );

            check_balance(
                &rollup_config,
                &mut tree,
                &block_info,
                a_id,
                sudt_id,
                &block_producer_address,
                fee,
            );
        }
    }
}

#[test]
fn test_insufficient_balance() {
    let mut tree = DummyState::default();
    let init_a_balance: u128 = 10000;

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();

    // init accounts
    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = tree.get_script_hash(a_id).expect("get script hash");
    let b_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = tree.get_script_hash(b_id).expect("get script hash");

    let block_info = new_block_info(0, 10, 0);

    // init balance for a
    tree.mint_sudt(sudt_id, to_short_address(&a_script_hash), init_a_balance)
        .expect("init balance");

    let b_address = to_short_address(&b_script_hash).to_vec();
    // transfer from A to B
    {
        let value = 10001u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(b_address.pack())
                    .amount(value.pack())
                    .build(),
            )
            .build();
        let err = run_contract(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_INSUFFICIENT_BALANCE);
    }
}

#[test]
fn test_transfer_to_non_exist_account() {
    let mut tree = DummyState::default();
    let init_a_balance: u128 = 10000;

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();

    // init accounts
    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = tree.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let b_address = [0x33u8; 20];

    let block_info = new_block_info(0, 10, 0);

    // init balance for a
    tree.mint_sudt(sudt_id, to_short_address(&a_script_hash), init_a_balance)
        .expect("init balance");

    // transfer from A to B
    {
        let value: u128 = 1000;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(b_address.pack())
                    .amount(value.pack())
                    .build(),
            )
            .build();
        let _run_result = run_contract(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
    }
}

#[test]
fn test_transfer_to_self() {
    let mut tree = DummyState::default();
    let init_a_balance: u128 = 10000;

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();

    // init accounts
    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = tree.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let a_address = to_short_address(&a_script_hash).to_vec();

    let block_producer_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = tree
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_producer_address = to_short_address(&block_producer_script_hash).to_vec();
    let block_producer_balance = 0;
    let block_info = new_block_info(block_producer_id, 10, 0);

    // init balance for a
    tree.mint_sudt(sudt_id, to_short_address(&a_script_hash), init_a_balance)
        .expect("init balance");

    // transfer from A to A, zero value
    {
        let value: u128 = 0;
        let fee: u128 = 0;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(a_address.pack())
                    .amount(value.pack())
                    .fee(fee.pack())
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            block_producer_script_hash,
            fee,
            a_script_hash,
            a_script_hash,
            value,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &block_producer_address,
            block_producer_balance,
        );
    }

    // transfer from A to A, normal value
    let fee: u128 = 20;
    {
        let value: u128 = 1000;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(a_address.pack())
                    .amount(value.pack())
                    .fee(fee.pack())
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            block_producer_script_hash,
            fee,
            a_script_hash,
            a_script_hash,
            value,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance - fee,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &block_producer_address,
            block_producer_balance + fee,
        );
    }

    // transfer from A to A, insufficient balance
    {
        let value: u128 = 100000;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(a_address.pack())
                    .amount(value.pack())
                    .build(),
            )
            .build();
        let err = run_contract(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_INSUFFICIENT_BALANCE);
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance - fee,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &block_producer_address,
            block_producer_balance + fee,
        );
    }
}

#[test]
fn test_transfer_to_self_overflow() {
    let mut tree = DummyState::default();
    let init_a_balance: u128 = u128::MAX - 1;

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();

    // init accounts
    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = tree.get_script_hash(a_id).expect("get script hash");
    // non-exist account id
    let a_address = to_short_address(&a_script_hash).to_vec();

    let block_producer_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([2u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let block_producer_script_hash = tree
        .get_script_hash(block_producer_id)
        .expect("get script hash");
    let block_producer_address = to_short_address(&block_producer_script_hash).to_vec();
    let block_producer_balance = 0;
    let block_info = new_block_info(block_producer_id, 10, 0);

    // init balance for a
    tree.mint_sudt(sudt_id, to_short_address(&a_script_hash), init_a_balance)
        .expect("init balance");

    // transfer from A to A, zero value
    {
        let value: u128 = 0;
        let fee: u128 = 0;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(a_address.pack())
                    .amount(value.pack())
                    .fee(fee.pack())
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            block_producer_script_hash,
            fee,
            a_script_hash,
            a_script_hash,
            value,
        );

        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &block_producer_address,
            block_producer_balance,
        );
    }

    // transfer from A to A, 1 value
    {
        let value: u128 = 1;
        let fee: u128 = 0;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(a_address.pack())
                    .amount(value.pack())
                    .fee(fee.pack())
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("run contract");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            block_producer_script_hash,
            fee,
            a_script_hash,
            a_script_hash,
            value,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &block_producer_address,
            block_producer_balance,
        );
    }

    // transfer from A to A, overflow balance
    {
        let value: u128 = 100000;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(a_address.pack())
                    .amount(value.pack())
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("ok");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            block_producer_script_hash,
            0,
            a_script_hash,
            a_script_hash,
            value,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &block_producer_address,
            block_producer_balance,
        );
    }

    // transfer from A to A with a large value
    {
        let value: u128 = u128::MAX - 1;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(a_address.pack())
                    .amount(value.pack())
                    .build(),
            )
            .build();
        let run_result = run_contract_get_result(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect("ok");
        assert_eq!(run_result.logs.len(), 2);
        check_transfer_logs(
            &run_result.logs,
            sudt_id,
            block_producer_script_hash,
            0,
            a_script_hash,
            a_script_hash,
            value,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &block_producer_address,
            block_producer_balance,
        );
    }
}

#[test]
fn test_transfer_overflow() {
    let mut tree = DummyState::default();
    let init_a_balance: u128 = 10000;
    let init_b_balance: u128 = u128::MAX;

    let rollup_config = RollupConfig::new_builder()
        .l2_sudt_validator_script_type_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
        .build();

    // init accounts
    let sudt_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let a_script_hash = tree.get_script_hash(a_id).expect("get script hash");
    let a_address = to_short_address(&a_script_hash).to_vec();
    let b_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([1u8; 20].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    let b_script_hash = tree.get_script_hash(b_id).expect("get script hash");

    let block_info = new_block_info(0, 10, 0);

    // init balance for a
    tree.mint_sudt(sudt_id, to_short_address(&a_script_hash), init_a_balance)
        .expect("init balance");
    tree.mint_sudt(sudt_id, to_short_address(&b_script_hash), init_b_balance)
        .expect("init balance");

    let b_address = to_short_address(&b_script_hash).to_vec();
    // transfer from A to B overflow
    {
        let value: u128 = 1000;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to(b_address.pack())
                    .amount(value.pack())
                    .build(),
            )
            .build();
        let err = run_contract(
            &rollup_config,
            &mut tree,
            a_id,
            sudt_id,
            args.as_bytes(),
            &block_info,
        )
        .expect_err("err");
        let err_code = match err {
            TransactionError::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, ERROR_AMOUNT_OVERFLOW);

        // check balance
        check_balance(
            &rollup_config,
            &mut tree,
            &block_info,
            a_id,
            sudt_id,
            &a_address,
            init_a_balance,
        );

        check_balance(
            &rollup_config,
            &mut tree,
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
    short_address: &[u8],
    expected_balance: u128,
) {
    // check balance
    let args = SUDTArgs::new_builder()
        .set(
            SUDTQuery::new_builder()
                .short_address(short_address.pack())
                .build(),
        )
        .build();
    let return_data = run_contract(
        &rollup_config,
        tree,
        sender_id,
        sudt_id,
        args.as_bytes(),
        &block_info,
    )
    .expect("execute");
    let balance = {
        let mut buf = [0u8; 16];
        buf.copy_from_slice(&return_data);
        u128::from_le_bytes(buf)
    };
    assert_eq!(balance, expected_balance);
}
