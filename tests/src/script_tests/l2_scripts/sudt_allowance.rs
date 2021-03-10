use crate::testing_tool::chain::build_backend_manage;

use super::{
    new_block_info, DummyChainStore, SUDT_ALLOWANCE_PROGRAM, SUDT_ALLOWANCE_PROGRAM_CODE_HASH,
};
use gw_common::state::{build_account_key, State};
use gw_common::H256;
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::Backend,
    dummy_state::DummyState,
    traits::StateExt,
    Generator, RollupContext,
};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{BlockInfo, RawL2Transaction, RollupConfig, Script},
    prelude::*,
};

#[test]
fn test_sudt_allowance() {
    let mut tree = DummyState::default();
    let chain_view = DummyChainStore;
    let from_id: u32 = 2;
    let rollup_config = RollupConfig::default();

    let contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_ALLOWANCE_PROGRAM_CODE_HASH.pack())
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
            validator: SUDT_ALLOWANCE_PROGRAM.clone(),
            generator: SUDT_ALLOWANCE_PROGRAM.clone(),
            validator_script_type_hash: SUDT_ALLOWANCE_PROGRAM_CODE_HASH.clone().into(),
        });
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage
            .register_lock_algorithm(H256::zero(), Box::new(AlwaysSuccess::default()));
        let rollup_context = RollupContext {
            rollup_config: Default::default(),
            rollup_script_hash: [42u8; 32].into(),
        };
        let generator = Generator::new(backend_manage, account_lock_manage, rollup_context);

        let block_info = new_block_info(0, 1, 0);
        for (sudt_id, owner_id, spender_id, amount) in vec![
            (1, 2, 3, 0xf343),
            (2, 3, 4, 0xfff343),
            (3, 4, 5, 0x13f343),
            (4, 5, 3, 0xabf343),
        ] {
            // test set allowance
            set_allowance(
                &generator,
                &chain_view,
                &mut tree,
                &block_info,
                from_id,
                contract_id,
                sudt_id,
                owner_id,
                spender_id,
                amount,
            );
            let key = build_raw_allowance_key(sudt_id, owner_id, spender_id);
            println!("key: {:?}", key);
            let mut expected_value = [0u8; 32];
            expected_value[0..16].copy_from_slice(&amount.to_le_bytes()[..]);
            assert_eq!(tree.get_raw(&key).unwrap(), H256::from(expected_value));

            // test get allowance
            let query_amount = get_allowance(
                &generator,
                &chain_view,
                &tree,
                &block_info,
                from_id,
                contract_id,
                sudt_id,
                owner_id,
                spender_id,
            );
            assert_eq!(amount, query_amount);
        }
    }
}

fn build_raw_allowance_key(sudt_id: u32, owner_id: u32, spender_id: u32) -> H256 {
    let mut key = [0u8; 32];
    key[0..8].copy_from_slice(b"allowanc");
    key[8..12].copy_from_slice(&owner_id.to_le_bytes()[..]);
    key[12..16].copy_from_slice(&spender_id.to_le_bytes()[..]);
    build_account_key(sudt_id, &key[..])
}

fn set_allowance(
    generator: &Generator,
    chain_view: &DummyChainStore,
    tree: &mut DummyState,
    block_info: &BlockInfo,
    from_id: u32,
    contract_id: u32,
    sudt_id: u32,
    owner_id: u32,
    spender_id: u32,
    amount: u128,
) {
    let mut args = [0u8; 29];
    let mut offset: usize = 0;
    args[offset] = 0xf1;
    offset += 1;
    args[offset..offset + 4].copy_from_slice(&sudt_id.to_le_bytes()[..]);
    offset += 4;
    args[offset..offset + 4].copy_from_slice(&owner_id.to_le_bytes()[..]);
    offset += 4;
    args[offset..offset + 4].copy_from_slice(&spender_id.to_le_bytes()[..]);
    offset += 4;
    args[offset..offset + 16].copy_from_slice(&amount.to_le_bytes()[..]);
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_id.pack())
        .args(Bytes::from(args.to_vec()).pack())
        .build();
    let run_result = generator
        .execute_transaction(chain_view, tree, block_info, &raw_tx)
        .expect("construct");
    tree.apply_run_result(&run_result).expect("update state");
    println!("result {:?}", run_result);
}

fn get_allowance(
    generator: &Generator,
    chain_view: &DummyChainStore,
    tree: &DummyState,
    block_info: &BlockInfo,
    from_id: u32,
    contract_id: u32,
    sudt_id: u32,
    owner_id: u32,
    spender_id: u32,
) -> u128 {
    let mut args = [0u8; 13];
    let mut offset: usize = 0;
    args[offset] = 0xf2;
    offset += 1;
    args[offset..offset + 4].copy_from_slice(&sudt_id.to_le_bytes()[..]);
    offset += 4;
    args[offset..offset + 4].copy_from_slice(&owner_id.to_le_bytes()[..]);
    offset += 4;
    args[offset..offset + 4].copy_from_slice(&spender_id.to_le_bytes()[..]);
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_id.pack())
        .args(Bytes::from(args.to_vec()).pack())
        .build();
    let run_result = generator
        .execute_transaction(chain_view, tree, block_info, &raw_tx)
        .expect("construct");
    let amount = {
        let mut buf = [0u8; 16];
        buf.copy_from_slice(&run_result.return_data);
        u128::from_le_bytes(buf)
    };
    println!("result {:?}", run_result);
    amount
}
