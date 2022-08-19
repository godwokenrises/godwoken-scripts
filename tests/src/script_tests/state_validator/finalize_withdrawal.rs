use std::collections::HashMap;

use ckb_types::core::Cycle;
use gw_common::h256_ext::H256Ext;
use gw_common::merkle_utils::{calculate_ckb_merkle_root, ckb_merkle_leaf_hash, CBMT};
use gw_common::smt::SMT;
use gw_common::sparse_merkle_tree::default_store::DefaultStore;
use gw_common::H256;
use gw_types::bytes::Bytes;
use gw_types::core::ScriptHashType;
use gw_types::packed::{
    BlockMerkleState, CKBMerkleProof, CellOutput, CustodianLockArgs, GlobalState, L2Block,
    LastFinalizedWithdrawal, RawL2Block, RawL2BlockWithdrawals, RawL2BlockWithdrawalsVec,
    RawWithdrawalRequest, RollupAction, RollupActionUnion, RollupConfig, RollupFinalizeWithdrawal,
    Script, SubmitWithdrawals, WithdrawalRequest, WithdrawalRequestVec, WitnessArgs,
};
use gw_types::prelude::{Builder, Entity, Pack, PackVec, Unpack};

use crate::script_tests::utils::conversion::{CKBTypeIntoExt, ToCKBType, ToGWType};
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::{
    build_simple_tx, random_always_success_script, random_out_point,
};
use crate::script_tests::utils::rollup::{
    build_rollup_locked_cell, calculate_state_validator_type_id, CellContext,
};
use crate::testing_tool::programs::{
    ALWAYS_SUCCESS_PROGRAM, CUSTODIAN_LOCK_PROGRAM, STATE_VALIDATOR_CODE_HASH,
};

mod last_finalized_withdrawal;
mod user_withdrawal_cells;

const FINALITY_BLOCKS: u64 = 10u64;
const BLOCK_NO_WITHDRAWAL: u32 = u32::MAX;
const BLOCK_ALL_WITHDRAWALS: u32 = u32::MAX - 1;
const CKB: u64 = 10u64.pow(8);

#[test]
fn test_rollup_finalize_withdrawal() {
    init_env_log();

    let test_case = TestCaseArgs::builder()
        .push_withdrawal(1, 1000 * CKB, 100)
        .push_withdrawal(1 + FINALITY_BLOCKS, 1000 * CKB, 100)
        .prev_last_finalized_withdrawal(0, BLOCK_NO_WITHDRAWAL)
        .post_last_finalized_withdrawal(1, BLOCK_ALL_WITHDRAWALS)
        .build();

    test_case.verify().expect("success");
}

struct WithdrawalRequestArgs {
    block_number: u64,
    capacity: u64,
    sudt_amount: u128,
    sudt_type: Option<Script>,
    owner_lock: Script,
}

impl WithdrawalRequestArgs {
    fn to_req(&self) -> WithdrawalRequest {
        let mut raw_builder = RawWithdrawalRequest::new_builder()
            .capacity(self.capacity.pack())
            .amount(self.sudt_amount.pack())
            .owner_lock_hash(self.owner_lock.hash().pack());

        if let Some(sudt_type) = self.sudt_type.as_ref() {
            raw_builder = raw_builder.sudt_script_hash(sudt_type.hash().pack());
        }

        WithdrawalRequest::new_builder()
            .raw(raw_builder.build())
            .build()
    }
}

struct ContractDep {
    output: CellOutput,
    data: Bytes,
    type_: Script,
}

impl ContractDep {
    fn new(data: Bytes) -> Self {
        let type_ = random_always_success_script().to_gw();

        let dummy_output = CellOutput::new_builder()
            .capacity(1u64.pack())
            .type_(Some(type_.clone()).pack())
            .lock(random_always_success_script().to_gw())
            .build();

        let capacity = dummy_output.occupied_capacity(data.len()).unwrap();
        let output = dummy_output.as_builder().capacity(capacity.pack()).build();

        ContractDep {
            output,
            data,
            type_,
        }
    }
}

struct TestCaseArgsBuilder {
    custodian_lock: ContractDep,
    sudt_type: ContractDep,

    rollup_type: Script,
    rollup_config: RollupConfig,

    prev_last_finalized_withdrawal: LastFinalizedWithdrawal,
    post_last_finalized_withdrawal: LastFinalizedWithdrawal,

    withdrawals: Vec<WithdrawalRequestArgs>,
}

impl TestCaseArgsBuilder {
    fn new() -> Self {
        let custodian_lock = ContractDep::new(CUSTODIAN_LOCK_PROGRAM.clone());
        let sudt_type = ContractDep::new(ALWAYS_SUCCESS_PROGRAM.clone());

        let rollup_type = {
            let input_out_point = random_out_point();
            let type_id = calculate_state_validator_type_id(input_out_point.clone());

            Script::new_builder()
                .code_hash((*STATE_VALIDATOR_CODE_HASH).pack())
                .hash_type(ScriptHashType::Data.into())
                .args(type_id.to_vec().pack())
                .build()
        };
        let rollup_config = RollupConfig::new_builder()
            .custodian_script_type_hash(custodian_lock.type_.hash().pack())
            .l1_sudt_script_type_hash(sudt_type.type_.hash().pack())
            .finality_blocks(FINALITY_BLOCKS.pack())
            .build();

        TestCaseArgsBuilder {
            custodian_lock,
            sudt_type,

            rollup_type,
            rollup_config,

            prev_last_finalized_withdrawal: Default::default(),
            post_last_finalized_withdrawal: Default::default(),

            withdrawals: Default::default(),
        }
    }

    fn generate_withdrawal(
        &self,
        block_number: u64,
        capacity: u64,
        sudt_amount: u128,
    ) -> WithdrawalRequestArgs {
        let sudt_type = if sudt_amount > 0 {
            let sudt_type = Script::new_builder()
                .code_hash(self.rollup_config.l1_sudt_script_type_hash())
                .hash_type(ScriptHashType::Type.into())
                .args(rand::random::<[u8; 32]>().to_vec().pack())
                .build();
            Some(sudt_type)
        } else {
            None
        };

        WithdrawalRequestArgs {
            block_number,
            capacity,
            sudt_amount,
            sudt_type,
            owner_lock: random_always_success_script().to_gw(),
        }
    }

    fn prev_last_finalized_withdrawal(mut self, block_number: u64, index: u32) -> Self {
        let prev = LastFinalizedWithdrawal::new_builder()
            .block_number(block_number.pack())
            .withdrawal_index(index.pack())
            .build();

        self.prev_last_finalized_withdrawal = prev;
        self
    }

    fn post_last_finalized_withdrawal(mut self, block_number: u64, index: u32) -> Self {
        let post = LastFinalizedWithdrawal::new_builder()
            .block_number(block_number.pack())
            .withdrawal_index(index.pack())
            .build();

        self.post_last_finalized_withdrawal = post;
        self
    }

    fn push_withdrawals(mut self, withdrawals: Vec<WithdrawalRequestArgs>) -> Self {
        self.withdrawals.extend(withdrawals);
        self
    }

    fn push_withdrawal(self, block_number: u64, capacity: u64, sudt_amount: u128) -> Self {
        let withdrawal = self.generate_withdrawal(block_number, capacity, sudt_amount);
        self.push_withdrawals(vec![withdrawal])
    }

    fn build(self) -> TestCaseArgs {
        let block_withdrawals = BlockWithdrawals::from_args(self.withdrawals);

        let (user_withdrawal_cells, finalize_withdrawal) = block_withdrawals
            .generate_finalize_withdrawals(
                &self.prev_last_finalized_withdrawal,
                &self.post_last_finalized_withdrawal,
            );

        let (input_custodian_cells, output_custodian_cells): (Vec<_>, Vec<Option<_>>) =
            user_withdrawal_cells
                .iter()
                .map(UserWithdrawalCell::generate_custodians)
                .unzip();
        let output_custodian_cells: Vec<CustodianCell> = output_custodian_cells
            .into_iter()
            .filter_map(|c| c)
            .collect();

        let user_withdrawal_cells: HashMap<H256, Vec<UserWithdrawalCell>> = user_withdrawal_cells
            .into_iter()
            .fold(HashMap::new(), |mut map, wc| {
                let withdrawals_mut = map.entry(wc.lock.hash().into()).or_insert(vec![]);
                withdrawals_mut.push(wc);
                map
            });

        let prev_global_state = {
            let tip_block_number: u64 = {
                let b = block_withdrawals.blocks.last().unwrap().raw();
                b.number().unpack()
            };
            let last_finalized_block_number =
                tip_block_number.saturating_sub(self.rollup_config.finality_blocks().unpack());
            println!("tip block number {}", tip_block_number);
            println!("last finalized block {}", last_finalized_block_number);

            GlobalState::new_builder()
                .rollup_config_hash(self.rollup_config.hash().pack())
                .block(block_withdrawals.block_merkle_state())
                .last_finalized_block_number(last_finalized_block_number.pack())
                .last_finalized_withdrawal(self.prev_last_finalized_withdrawal)
                .version(2u8.into())
                .build()
        };

        let post_global_state = { prev_global_state.clone() }
            .as_builder()
            .last_finalized_withdrawal(self.post_last_finalized_withdrawal)
            .build();

        let rollup_type_hash: H256 = self.rollup_type.hash().into();
        let rollup_cell = {
            let dummy_output = CellOutput::new_builder()
                .capacity(u64::MAX.pack())
                .type_(Some(self.rollup_type).pack())
                .lock(random_always_success_script().to_gw())
                .build();

            let capacity = dummy_output
                .occupied_capacity(prev_global_state.as_bytes().len())
                .unwrap();

            dummy_output.as_builder().capacity(capacity.pack()).build()
        };

        TestCaseArgs {
            rollup_type_hash,
            rollup_config: self.rollup_config,
            prev_global_state,
            post_global_state,
            custodian_lock: self.custodian_lock,
            sudt_type: self.sudt_type,
            rollup_cell,
            input_custodian_cells,
            user_withdrawal_cells,
            output_custodian_cells,
            finalize_withdrawal,
        }
    }
}

#[derive(Default, Debug)]
struct CustodianCell {
    capacity: u64,
    sudt_amount: u128,
    type_: Option<Script>,
    lock_args: CustodianLockArgs,
}

impl CustodianCell {
    fn to_output_data(
        &self,
        rollup_type_hash: H256,
        custodian_lock_type_hash: H256,
    ) -> (CellOutput, Bytes) {
        let output = build_rollup_locked_cell(
            &(rollup_type_hash.into()),
            &(custodian_lock_type_hash.into()),
            self.capacity,
            self.lock_args.as_bytes(),
        )
        .to_gw();

        let data = self.sudt_amount.pack().as_bytes();
        let output = output.as_builder().type_(self.type_.clone().pack()).build();

        (output, data)
    }
}

#[derive(Clone, Debug)]
struct UserWithdrawalCell {
    capacity: u64,
    sudt_amount: u128,
    type_: Option<Script>,
    lock: Script,
}

impl UserWithdrawalCell {
    fn to_output_data(&self) -> (CellOutput, Bytes) {
        let data = self.sudt_amount.pack().as_bytes();
        let output = CellOutput::new_builder()
            .capacity(self.capacity.pack())
            .type_(self.type_.clone().pack())
            .lock(self.lock.clone())
            .build();

        (output, data)
    }

    fn generate_custodians(&self) -> (CustodianCell, Option<CustodianCell>) {
        let input_capacity = self.capacity.saturating_mul(2);
        let input_sudt_amount = self.sudt_amount.saturating_mul(2);
        let input_custodian = CustodianCell {
            capacity: input_capacity,
            sudt_amount: input_sudt_amount,
            type_: self.type_.clone(),
            lock_args: CustodianLockArgs::default(),
        };

        let output_capacity = input_capacity.checked_sub(self.capacity);
        let output_sudt_amount = input_sudt_amount.saturating_sub(self.sudt_amount);
        if output_sudt_amount > 0 {
            assert!(output_capacity > Some(200 * 10u64.pow(8)));
        }
        let output_custodian = output_capacity.map(|capacity| CustodianCell {
            capacity,
            sudt_amount: output_sudt_amount,
            type_: self.type_.clone(),
            lock_args: CustodianLockArgs::default(),
        });

        (input_custodian, output_custodian)
    }
}

struct BlockWithdrawals {
    blocks: Vec<L2Block>,
    block_withdrawals: HashMap<u64, Vec<UserWithdrawalCell>>,
}

impl BlockWithdrawals {
    fn from_args(mut withdrawals: Vec<WithdrawalRequestArgs>) -> Self {
        withdrawals.sort_unstable_by(|a, b| a.block_number.cmp(&b.block_number));

        let block_args = withdrawals
            .into_iter()
            .map(|args| (args.block_number, vec![args]))
            .collect::<HashMap<_, Vec<_>>>();

        let (mut blocks, block_withdrawals): (Vec<_>, HashMap<_, _>) = block_args
            .into_iter()
            .map(|(bn, args)| {
                let withdrawals = args.iter().map(|a| a.to_req());

                let witness_root = calculate_ckb_merkle_root(
                    { withdrawals.clone().enumerate() }
                        .map(|(i, r)| ckb_merkle_leaf_hash(i as u32, &r.witness_hash().into()))
                        .collect(),
                );

                let submit_withdrawals = SubmitWithdrawals::new_builder()
                    .withdrawal_witness_root(witness_root.unwrap().pack())
                    .withdrawal_count((withdrawals.clone().count() as u32).pack())
                    .build();

                let raw_block = RawL2Block::new_builder()
                    .number(bn.pack())
                    .submit_withdrawals(submit_withdrawals)
                    .build();

                let block = L2Block::new_builder()
                    .raw(raw_block)
                    .withdrawals(withdrawals.collect::<Vec<_>>().pack())
                    .build();

                let user_withdrawal_cells = args.into_iter().map(|w| UserWithdrawalCell {
                    capacity: w.capacity,
                    sudt_amount: w.sudt_amount,
                    type_: w.sudt_type,
                    lock: w.owner_lock,
                });

                (block, (bn, user_withdrawal_cells.collect::<Vec<_>>()))
            })
            .unzip();

        blocks.sort_unstable_by(|a, b| a.raw().number().unpack().cmp(&b.raw().number().unpack()));

        BlockWithdrawals {
            blocks,
            block_withdrawals,
        }
    }

    fn block_merkle_state(&self) -> BlockMerkleState {
        let last_block_number = self.blocks.last().unwrap().raw().number().unpack();

        BlockMerkleState::new_builder()
            .merkle_root(self.block_smt().root().pack())
            .count(last_block_number.saturating_add(1).pack())
            .build()
    }

    fn generate_finalize_withdrawals(
        &self,
        prev_last_finalized_withdrawal: &LastFinalizedWithdrawal,
        post_last_finalized_withdrawal: &LastFinalizedWithdrawal,
    ) -> (Vec<UserWithdrawalCell>, RollupFinalizeWithdrawal) {
        let (prev_block_number, prev_wth_idx): (u64, u32) = (
            prev_last_finalized_withdrawal.block_number().unpack(),
            prev_last_finalized_withdrawal.withdrawal_index().unpack(),
        );
        let (post_block_number, post_wth_idx): (u64, u32) = (
            post_last_finalized_withdrawal.block_number().unpack(),
            post_last_finalized_withdrawal.withdrawal_index().unpack(),
        );

        assert!(prev_block_number <= post_block_number);
        assert!(prev_block_number <= self.blocks.first().unwrap().raw().number().unpack());
        assert!(post_block_number <= self.blocks.last().unwrap().raw().number().unpack());

        let prev_block_wths = {
            let prev = self.block_withdrawals.get(&prev_block_number);
            prev.cloned().unwrap_or_default()
        };
        let post_block_wths = self.block_withdrawals.get(&post_block_number).unwrap();

        let assert_idx = |idx, block_wths: &[UserWithdrawalCell]| {
            let valid = BLOCK_ALL_WITHDRAWALS == idx
                || (0 != block_wths.len() && idx as usize <= block_wths.len().saturating_sub(1))
                || (0 == block_wths.len() && BLOCK_NO_WITHDRAWAL == idx);
            assert!(valid);
        };
        assert_idx(prev_wth_idx, &prev_block_wths);
        assert_idx(post_wth_idx, post_block_wths);

        let block_smt = self.block_smt();
        let block_range =
            if BLOCK_NO_WITHDRAWAL == prev_wth_idx || BLOCK_ALL_WITHDRAWALS == prev_wth_idx {
                (prev_block_number + 1)..=post_block_number
            } else {
                prev_block_number..=post_block_number
            };
        let key_leaf_vec = block_range
            .map(|bn| (H256::from_u64(bn), H256::zero()))
            .collect::<Vec<_>>();
        let block_proof = block_smt
            .merkle_proof(key_leaf_vec.iter().map(|kv| kv.0).collect())
            .unwrap()
            .compile(key_leaf_vec)
            .unwrap();

        fn build_block_withdrawals(
            l2block: &L2Block,
            withdrawal_cells: &[UserWithdrawalCell],
            range: Option<(u32, u32)>,
        ) -> (Vec<UserWithdrawalCell>, RawL2BlockWithdrawals) {
            let (withdrawals, withdrawals_proof, withdrawal_cells) = match range {
                Some((start, end)) => {
                    let wths = l2block.withdrawals().into_iter().enumerate();
                    let target_wths =
                        wths.filter(|(idx, _w)| idx >= &(start as usize) && idx <= &(end as usize));

                    let (target_wths, (indices, leaves)): (Vec<_>, (Vec<_>, Vec<_>)) = target_wths
                        .map(|(i, w)| {
                            let hash: H256 = w.witness_hash().into();
                            (w, (i as u32, ckb_merkle_leaf_hash(i as u32, &hash)))
                        })
                        .unzip();

                    let proof = CBMT::build_merkle_proof(&leaves, &indices).unwrap();
                    let cbmt_proof = CKBMerkleProof::new_builder()
                        .lemmas(proof.lemmas().pack())
                        .indices(proof.indices().pack())
                        .build();

                    let cells = withdrawal_cells.iter().enumerate().filter_map(|(i, w)| {
                        if i >= start as usize && i <= end as usize {
                            Some(w.to_owned())
                        } else {
                            None
                        }
                    });

                    (target_wths.pack(), cbmt_proof, cells.collect())
                }
                None => (
                    WithdrawalRequestVec::default(),
                    CKBMerkleProof::default(),
                    vec![],
                ),
            };

            let block_withdrawals = RawL2BlockWithdrawals::new_builder()
                .raw(l2block.raw())
                .withdrawals(withdrawals)
                .withdrawal_proof(withdrawals_proof)
                .build();

            (withdrawal_cells, block_withdrawals)
        }

        let finalize_blocks = self.blocks.iter().filter(|b| {
            let bn = b.raw().number().unpack();
            prev_block_number <= bn && bn <= post_block_number
        });

        let (user_withdrawal_cells, block_withdrawals_vec): (
            Vec<Vec<UserWithdrawalCell>>,
            Vec<RawL2BlockWithdrawals>,
        ) = finalize_blocks
            .filter_map(|b| {
                let bn = b.raw().number().unpack();
                let withdrawal_cells = self.block_withdrawals.get(&bn).unwrap();

                match bn {
                    block_number if prev_block_number == block_number => {
                        if prev_wth_idx < BLOCK_ALL_WITHDRAWALS {
                            if post_block_number == prev_block_number {
                                // Same block
                                Some(build_block_withdrawals(
                                    &b,
                                    withdrawal_cells,
                                    Some((prev_wth_idx + 1, post_wth_idx)),
                                ))
                            } else {
                                let end = b.withdrawals().len().saturating_sub(1) as u32;
                                Some(build_block_withdrawals(
                                    &b,
                                    withdrawal_cells,
                                    Some((prev_wth_idx + 1, end)),
                                ))
                            }
                        } else {
                            None
                        }
                    }
                    block_number
                        if block_number > prev_block_number && block_number < post_block_number =>
                    {
                        let end = b.withdrawals().len().saturating_sub(1) as u32;
                        Some(build_block_withdrawals(
                            &b,
                            withdrawal_cells,
                            Some((0, end)),
                        ))
                    }
                    block_number if post_block_number == block_number => {
                        if BLOCK_NO_WITHDRAWAL == post_wth_idx {
                            Some((
                                vec![],
                                RawL2BlockWithdrawals::new_builder().raw(b.raw()).build(),
                            ))
                        } else {
                            Some(build_block_withdrawals(
                                &b,
                                withdrawal_cells,
                                Some((0, post_wth_idx)),
                            ))
                        }
                    }
                    _ => unreachable!("unexpected block and last finalized withdrawal range"),
                }
            })
            .unzip();

        let finalize_witness = RollupFinalizeWithdrawal::new_builder()
            .block_withdrawals(
                RawL2BlockWithdrawalsVec::new_builder()
                    .set(block_withdrawals_vec)
                    .build(),
            )
            .block_proof(block_proof.0.pack())
            .build();

        (
            user_withdrawal_cells.into_iter().flatten().collect(),
            finalize_witness,
        )
    }

    fn block_smt(&self) -> SMT<DefaultStore<H256>> {
        let mut block_smt = SMT::new(H256::zero(), DefaultStore::default());
        let blocks = self.blocks.iter();

        let smt_key_leaves = blocks.map(|b| (H256::from(b.smt_key()), H256::from(b.hash())));
        block_smt.update_all(smt_key_leaves.collect()).unwrap();

        block_smt
    }
}

struct TestCaseArgs {
    rollup_type_hash: H256,
    rollup_config: RollupConfig,

    prev_global_state: GlobalState,
    post_global_state: GlobalState,

    // Deps
    custodian_lock: ContractDep,
    sudt_type: ContractDep,

    // Cells
    rollup_cell: CellOutput, // build_always_success_cell
    input_custodian_cells: Vec<CustodianCell>,
    user_withdrawal_cells: HashMap<H256, Vec<UserWithdrawalCell>>, // lock hash => user withdrawals
    output_custodian_cells: Vec<CustodianCell>,

    finalize_withdrawal: RollupFinalizeWithdrawal,
}

impl TestCaseArgs {
    fn builder() -> TestCaseArgsBuilder {
        TestCaseArgsBuilder::new()
    }

    fn verify(&self) -> Result<Cycle, ckb_error::Error> {
        let mut ctx = CellContext::new(&self.rollup_config, Default::default());

        // Set up contract deps
        let custodian_lock = &self.custodian_lock;
        ctx.custodian_lock_dep = ctx
            .insert_cell(custodian_lock.output.to_ckb(), custodian_lock.data.clone())
            .into_ext();
        ctx.l2_sudt_dep = ctx
            .insert_cell(self.sudt_type.output.to_ckb(), self.sudt_type.data.clone())
            .into_ext();

        let cell_deps = vec![
            ctx.rollup_config_dep.clone(),
            ctx.state_validator_dep.clone(),
            ctx.custodian_lock_dep.clone(),
            ctx.l2_sudt_dep.clone(),
            ctx.always_success_dep.clone(),
        ];

        let input_custodians = ctx
            .insert_cells(self.input_custodian_cells.iter().map(|c| {
                let (output, data) = c.to_output_data(
                    self.rollup_type_hash,
                    self.rollup_config.custodian_script_type_hash().unpack(),
                );
                (output.to_ckb(), data)
            }))
            .map(CKBTypeIntoExt::into_ext)
            .collect::<Vec<_>>();

        let output_custodians = self.output_custodian_cells.iter().map(|c| {
            c.to_output_data(
                self.rollup_type_hash,
                self.rollup_config.custodian_script_type_hash().unpack(),
            )
            .to_ckb()
        });
        let output_user_withdrawals = self
            .user_withdrawal_cells
            .values()
            .flatten()
            .map(|c| c.to_output_data().to_ckb());
        let (output_custodians_withdrawals, data_custodians_withdrawals): (Vec<_>, Vec<_>) =
            output_custodians.chain(output_user_withdrawals).unzip();

        let finalize_withdrawal_witness = {
            let rollup_finalize_witness = RollupAction::new_builder()
                .set(RollupActionUnion::RollupFinalizeWithdrawal(
                    self.finalize_withdrawal.clone(),
                ))
                .build();

            WitnessArgs::new_builder()
                .output_type(Some(rollup_finalize_witness.as_bytes()).pack())
                .build()
        };

        let tx = build_simple_tx(
            &mut ctx.inner,
            (self.rollup_cell.to_ckb(), self.prev_global_state.as_bytes()),
            Default::default(), // since
            (self.rollup_cell.to_ckb(), self.post_global_state.as_bytes()),
        )
        .as_advanced_builder()
        .witness(finalize_withdrawal_witness.as_bytes().to_ckb())
        .inputs(input_custodians)
        .outputs(output_custodians_withdrawals)
        .outputs_data(data_custodians_withdrawals)
        .cell_deps(cell_deps)
        .build();

        ctx.verify_tx(tx)
    }
}
