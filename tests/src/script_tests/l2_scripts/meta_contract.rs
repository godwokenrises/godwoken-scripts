use super::{new_block_info, run_contract};
use crate::testing_tool::chain::META_VALIDATOR_SCRIPT_TYPE_HASH;
use core::panic;
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    state::State,
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_generator::{
    dummy_state::DummyState,
    error::TransactionError,
    syscalls::error_codes::{GW_ERROR_DUPLICATED_SCRIPT_HASH, GW_SUDT_ERROR_INSUFFICIENT_BALANCE},
    traits::StateExt,
};
use gw_types::{
    core::ScriptHashType,
    offchain::RollupContext,
    packed::{CreateAccount, MetaContractArgs, RollupConfig, Script},
    prelude::*,
};

fn init_accounts(state: &mut DummyState, rollup_config: &RollupConfig) {
    let rollup_context = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: [42u8; 32].into(),
    };

    // setup meta_contract
    let meta_contract_id = state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(META_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                .args([0u8; 32].to_vec().pack())
                .hash_type(ScriptHashType::Type.into())
                .build(),
        )
        .expect("create account");
    assert_eq!(meta_contract_id, RESERVED_ACCOUNT_ID);

    // setup CKB simple UDT contract
    let ckb_sudt_script =
        gw_generator::sudt::build_l2_sudt_script(&rollup_context, &CKB_SUDT_SCRIPT_ARGS.into());
    let ckb_sudt_id = state.create_account_from_script(ckb_sudt_script).unwrap();
    assert_eq!(
        ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
        "ckb simple UDT account id"
    );
}

#[test]
fn test_meta_contract() {
    let mut tree = DummyState::default();
    let dummy_eoa_type_hash = [4u8; 32];
    let rollup_config = RollupConfig::new_builder()
        .allowed_eoa_type_hashes(vec![dummy_eoa_type_hash].pack())
        .build();
    init_accounts(&mut tree, &rollup_config);

    let a_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args([0u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let a_script_hash = a_script.hash();
    let a_id = tree
        .create_account_from_script(a_script)
        .expect("create account");
    tree.mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_script_hash[..20], 2000)
        .expect("mint CKB for account A to pay fee");

    let block_info = new_block_info(a_id, 1, 0);

    // create contract
    let contract_script = Script::new_builder()
        .code_hash(dummy_eoa_type_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args([42u8; 33].pack())
        .build();
    let args = MetaContractArgs::new_builder()
        .set(
            CreateAccount::new_builder()
                .script(contract_script.clone())
                .fee(1000u64.pack())
                .build(),
        )
        .build();
    let sender_nonce = tree.get_nonce(a_id).unwrap();
    let return_data = run_contract(
        &rollup_config,
        &mut tree,
        a_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &block_info,
    )
    .expect("execute");
    let new_sender_nonce = tree.get_nonce(a_id).unwrap();
    assert_eq!(sender_nonce + 1, new_sender_nonce, "nonce should increased");
    let account_id = {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&return_data);
        u32::from_le_bytes(buf)
    };
    assert_ne!(account_id, 0);

    let script_hash = tree.get_script_hash(account_id).expect("get script hash");
    assert_ne!(script_hash, H256::zero(), "script hash must exists");
    assert_eq!(
        script_hash,
        contract_script.hash().into(),
        "script hash must according to create account"
    );
}

#[test]
fn test_duplicated_script_hash() {
    let mut tree = DummyState::default();
    let rollup_config = RollupConfig::default();
    init_accounts(&mut tree, &rollup_config);

    let a_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args([0u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let a_script_hash = a_script.hash();
    let a_id = tree
        .create_account_from_script(a_script)
        .expect("create account");
    tree.mint_sudt(CKB_SUDT_ACCOUNT_ID, &a_script_hash[..20], 1000)
        .expect("mint CKB for account A to pay fee");

    let block_info = new_block_info(a_id, 1, 0);

    // create contract
    let contract_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args(vec![42].pack())
        .hash_type(ScriptHashType::Type.into())
        .build();

    let _id = tree
        .create_account_from_script(contract_script.clone())
        .expect("create account");

    // should return duplicated script hash
    let args = MetaContractArgs::new_builder()
        .set(
            CreateAccount::new_builder()
                .script(contract_script.clone())
                .fee(1000u64.pack())
                .build(),
        )
        .build();
    let err = run_contract(
        &rollup_config,
        &mut tree,
        a_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &block_info,
    )
    .unwrap_err();
    let err_code = match err {
        TransactionError::InvalidExitCode(code) => code,
        err => panic!("unexpected {:?}", err),
    };
    assert_eq!(err_code, GW_ERROR_DUPLICATED_SCRIPT_HASH);
}

#[test]
fn test_insufficient_balance_to_pay_fee() {
    let mut state = DummyState::default();
    let dummy_eoa_type_hash = [4u8; 32];
    let rollup_config = RollupConfig::new_builder()
        .allowed_eoa_type_hashes(vec![dummy_eoa_type_hash].pack())
        .build();
    init_accounts(&mut state, &rollup_config);

    let from_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args([0u8; 20].to_vec().pack())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let from_script_hash = from_script.hash();
    let from_id = state
        .create_account_from_script(from_script)
        .expect("create account");

    // create contract
    let contract_script = Script::new_builder()
        .code_hash(dummy_eoa_type_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args([42u8; 52].pack())
        .build();
    let args = MetaContractArgs::new_builder()
        .set(
            CreateAccount::new_builder()
                .script(contract_script.clone())
                .fee(1000u64.pack())
                .build(),
        )
        .build();
    let err = run_contract(
        &rollup_config,
        &mut state,
        from_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &new_block_info(from_id, 1, 0),
    )
    .unwrap_err();
    let err_code = match err {
        TransactionError::InvalidExitCode(code) => code,
        err => panic!("unexpected {:?}", err),
    };
    assert_eq!(
        err_code,
        gw_generator::syscalls::error_codes::GW_SUDT_ERROR_INSUFFICIENT_BALANCE
    );

    state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &from_script_hash[..20], 999)
        .expect("mint CKB for account A to pay fee");
    let err = run_contract(
        &rollup_config,
        &mut state,
        from_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &new_block_info(from_id, 2, 0),
    )
    .unwrap_err();
    let err_code = match err {
        TransactionError::InvalidExitCode(code) => code,
        err => panic!("unexpected {:?}", err),
    };
    assert_eq!(err_code, GW_SUDT_ERROR_INSUFFICIENT_BALANCE);

    state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &from_script_hash[..20], 1000)
        .expect("mint CKB for account A to pay fee");
    let _return_data = run_contract(
        &rollup_config,
        &mut state,
        from_id,
        RESERVED_ACCOUNT_ID,
        args.as_bytes(),
        &new_block_info(from_id, 3, 0),
    )
    .expect("contract created successful");
}
