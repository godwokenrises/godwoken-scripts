use ckb_types::prelude::Entity;
use gw_common::{h256_ext::H256Ext, H256};
use gw_types::packed::LastFinalizedWithdrawal;
use gw_types::prelude::{Builder, Pack, Unpack};

use super::{TestCase, BLOCK_ALL_WITHDRAWALS, CKB, FINALITY_BLOCKS};

use super::ERROR_MERKLE_PROOF;
const ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL: i8 = 46;
const ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS: i8 = 47;

#[test]
#[ignore = "unable to pass block proof verification"]
fn test_witness_empty_block_withdrawals() {
    unreachable!()
}

#[test]
fn test_invalid_block_number() {
    let mut test_case = TestCase::sample_case();

    let prev_last_finalized_withdrawal = test_case.prev_global_state.last_finalized_withdrawal();
    let post_last_finalized_withdrawal = test_case.post_global_state.last_finalized_withdrawal();

    let prev_block_number = prev_last_finalized_withdrawal.block_number().unpack();
    assert!(prev_block_number > 0);

    // post block < prev block
    {
        let err_post_last_finalized_withdrawal = post_last_finalized_withdrawal
            .clone()
            .as_builder()
            .block_number(prev_block_number.saturating_sub(1).pack())
            .build();

        let mut test_case = test_case.clone();
        test_case.post_global_state = test_case
            .post_global_state
            .as_builder()
            .last_finalized_withdrawal(err_post_last_finalized_withdrawal)
            .build();

        expect_err!(test_case, ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL);
    }

    // post block > last finalized block number
    let last_finalized_block_number = test_case
        .post_global_state
        .last_finalized_block_number()
        .unpack();

    let err_post_last_finalized_withdrawal = post_last_finalized_withdrawal
        .as_builder()
        .block_number(last_finalized_block_number.saturating_add(1).pack())
        .build();

    test_case.post_global_state = test_case
        .post_global_state
        .as_builder()
        .last_finalized_withdrawal(err_post_last_finalized_withdrawal)
        .build();

    expect_err!(test_case, ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL);
}

mod same_block {
    use super::*;

    fn sample_case() -> TestCase {
        TestCase::builder()
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .last_finalized_block(1)
            .prev_last_finalized_withdrawal(1, 0)
            .post_last_finalized_withdrawal(1, 2)
            .build()
    }

    #[test]
    fn test_valid_case() {
        let test_case = sample_case();
        test_case.verify().expect("pass");

        // prev index == BLOCK_ALL_WITHDRAWAL
        {
            let mut test_case = test_case.clone();
            let modified_prev_last_finalized_withdrawal = test_case
                .prev_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(BLOCK_ALL_WITHDRAWALS.pack())
                .build();

            test_case.prev_global_state = test_case
                .prev_global_state
                .as_builder()
                .last_finalized_withdrawal(modified_prev_last_finalized_withdrawal)
                .build();

            test_case.verify().expect("pass");
        }

        // post last withdrawal index
        {
            let last_withdrawal_index = {
                let withdrawals = test_case.builder.withdrawals.get(&1).unwrap();
                withdrawals.len().saturating_sub(1) as u32
            };

            let test_case = test_case
                .clone()
                .into_builder()
                .post_last_finalized_withdrawal(1, last_withdrawal_index)
                .build();

            test_case.verify().expect("pass");
        }

        // post index == prev index (BLOCK_ALL_WITHDRAWALS == last withdrawal index)
        {
            let mut test_case = test_case
                .clone()
                .into_builder()
                .push_withdrawal(3, 100, 0)
                .push_withdrawal(3, 100, 0)
                .push_withdrawal(3, 100, 0)
                .last_finalized_block(3)
                .prev_last_finalized_withdrawal(3, 1)
                .post_last_finalized_withdrawal(3, 2)
                .build();
            test_case.verify().expect("pass");

            let err_prev_last_finalized_withdrawal = test_case
                .prev_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(2u32.pack())
                .build();

            test_case.prev_global_state = test_case
                .prev_global_state
                .as_builder()
                .last_finalized_withdrawal(err_prev_last_finalized_withdrawal)
                .build();

            let err_post_last_finalized_withdrawal = test_case
                .post_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(BLOCK_ALL_WITHDRAWALS.pack())
                .build();

            test_case.post_global_state = test_case
                .post_global_state
                .as_builder()
                .last_finalized_withdrawal(err_post_last_finalized_withdrawal)
                .build();

            test_case.verify().expect("pass");
        }

        // post BLOCK_ALL_WITHDRAWALS
        let test_case = test_case
            .into_builder()
            .post_last_finalized_withdrawal(1, BLOCK_ALL_WITHDRAWALS)
            .build();

        test_case.verify().expect("pass");
    }

    #[test]
    fn test_witness_submit_extra_block_withdrawals() {
        // Create a test case include block #2
        let mut test_case = sample_case()
            .into_builder()
            .push_withdrawal(2, 100 * CKB, 0)
            .post_last_finalized_withdrawal(2, BLOCK_ALL_WITHDRAWALS)
            .build();

        // Modify post last to block #1
        let err_post_last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
            .block_number(1.pack())
            .withdrawal_index(BLOCK_ALL_WITHDRAWALS.pack())
            .build();

        test_case.post_global_state = test_case
            .post_global_state
            .as_builder()
            .last_finalized_withdrawal(err_post_last_finalized_withdrawal)
            .build();

        expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
    }

    #[test]
    #[ignore = "already checked before enter this logic"]
    fn test_invalid_prev_index_withdrawal_index_no_withdrawal() {
        unreachable!()
    }

    #[test]
    fn test_invalid_index() {
        let test_case = sample_case();

        // post index == WithdrawalIndex::NoWithdrawal
        {
            let mut test_case = test_case
                .clone()
                .into_builder()
                .push_empty_block(2)
                .last_finalized_block(2)
                .prev_last_finalized_withdrawal(1, BLOCK_ALL_WITHDRAWALS)
                .post_last_finalized_withdrawal(2, BLOCK_ALL_WITHDRAWALS)
                .build();
            test_case.verify().expect("pass");

            // set withdrawal index, so prev index won't set to WithdrawalIndex::NoWithdrawal
            let err_prev_last_finalized_withdrawal = test_case
                .prev_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .block_number(2.pack())
                .withdrawal_index(0u32.pack())
                .build();

            test_case.prev_global_state = test_case
                .prev_global_state
                .as_builder()
                .last_finalized_withdrawal(err_prev_last_finalized_withdrawal)
                .build();

            expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
        }

        // post index < prev index
        {
            let mut test_case = test_case.clone();

            let post_index: u32 = test_case
                .post_global_state
                .last_finalized_withdrawal()
                .withdrawal_index()
                .unpack();

            let err_prev_last_finalized_withdrawal = test_case
                .prev_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(post_index.saturating_add(1).pack())
                .build();

            test_case.prev_global_state = test_case
                .prev_global_state
                .as_builder()
                .last_finalized_withdrawal(err_prev_last_finalized_withdrawal)
                .build();

            expect_err!(test_case, ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL);
        }

        // prev index > last withdrawal index
        {
            let mut test_case = test_case.clone();

            let err_prev_last_finalized_withdrawal = test_case
                .prev_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(99u32.pack())
                .build();

            test_case.prev_global_state = test_case
                .prev_global_state
                .as_builder()
                .last_finalized_withdrawal(err_prev_last_finalized_withdrawal)
                .build();

            let err_post_last_finalized_withdrawal = test_case
                .post_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(100u32.pack())
                .build();

            test_case.post_global_state = test_case
                .post_global_state
                .as_builder()
                .last_finalized_withdrawal(err_post_last_finalized_withdrawal)
                .build();

            expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
        }

        // post index > last withdrawal index
        {
            let mut test_case = test_case;

            let err_post_last_finalized_withdrawal = test_case
                .post_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(100u32.pack())
                .build();

            test_case.post_global_state = test_case
                .post_global_state
                .as_builder()
                .last_finalized_withdrawal(err_post_last_finalized_withdrawal)
                .build();

            expect_err!(test_case, ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL);
        }
    }

    #[test]
    fn test_witness_submit_wrong_block() {
        let mut test_case = sample_case()
            .into_builder()
            .push_withdrawal(2, 100 * CKB, 0)
            .push_withdrawal(2, 100 * CKB, 0)
            .last_finalized_block(2)
            .prev_last_finalized_withdrawal(2, 0)
            .post_last_finalized_withdrawal(2, BLOCK_ALL_WITHDRAWALS)
            .build();
        test_case.verify().expect("pass");

        // Replace with block #1 finalize withdrawal
        test_case.finalize_withdrawal = {
            let prev = LastFinalizedWithdrawal::new_builder()
                .block_number(1.pack())
                .withdrawal_index(0u32.pack())
                .build();

            let post = LastFinalizedWithdrawal::new_builder()
                .block_number(1.pack())
                .withdrawal_index(BLOCK_ALL_WITHDRAWALS.pack())
                .build();

            let (_, finalize_withdrawal) = test_case
                .block_withdrawals
                .generate_finalize_withdrawals(&prev, &post);

            finalize_withdrawal
        };

        expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
    }
}

mod across_blocks {
    use gw_types::packed::{
        RawL2BlockWithdrawals, RawL2BlockWithdrawalsVec, RollupFinalizeWithdrawal,
    };

    use super::*;

    fn sample_case() -> TestCase {
        TestCase::builder()
            .push_empty_block(0)
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(2, 301 * CKB, 999)
            .push_withdrawal(2, 300 * CKB, 1)
            .push_empty_block(3)
            .push_withdrawal(4, 1000 * CKB, 0)
            .push_withdrawal(4, 1000 * CKB, 100)
            .last_finalized_block(4)
            .prev_last_finalized_withdrawal(1, 0)
            .post_last_finalized_withdrawal(4, 0)
            .build()
    }

    #[test]
    fn test_valid_cases() {
        let test_case = sample_case();
        test_case.verify().expect("pass");

        // prev BLOCK_ALL_WITHDRAWALS (no withdrawal)
        {
            let test_case = test_case
                .clone()
                .into_builder()
                .prev_last_finalized_withdrawal(0, BLOCK_ALL_WITHDRAWALS)
                .build();
            test_case.verify().expect("pass");
        }

        // prev BLOCK_ALL_WITHDRAWALS
        {
            let test_case = test_case
                .clone()
                .into_builder()
                .prev_last_finalized_withdrawal(1, BLOCK_ALL_WITHDRAWALS)
                .build();
            test_case.verify().expect("pass");
        }

        // prev last withdrawal index
        {
            let last_withdrawal_index = {
                let withdrawals = test_case.builder.withdrawals.get(&1).unwrap();
                withdrawals.len().saturating_sub(1) as u32
            };

            let test_case = test_case
                .clone()
                .into_builder()
                .prev_last_finalized_withdrawal(1, last_withdrawal_index)
                .build();
            test_case.verify().expect("pass");
        }

        // post BLOCK_ALL_WITHDRAWAL (no withdrawal)
        {
            let test_case = test_case
                .clone()
                .into_builder()
                .post_last_finalized_withdrawal(3, BLOCK_ALL_WITHDRAWALS)
                .build();
            test_case.verify().expect("pass");
        }

        // post BLOCK_ALL_WITHDRAWALS
        {
            let test_case = test_case
                .clone()
                .into_builder()
                .post_last_finalized_withdrawal(4, BLOCK_ALL_WITHDRAWALS)
                .build();
            test_case.verify().expect("pass");
        }

        // post last withdrawal index
        {
            let last_withdrawal_index = {
                let withdrawals = test_case.builder.withdrawals.get(&4).unwrap();
                withdrawals.len().saturating_sub(1) as u32
            };

            let test_case = test_case
                .into_builder()
                .post_last_finalized_withdrawal(4, last_withdrawal_index)
                .build();
            test_case.verify().expect("pass");
        }
    }

    #[test]
    #[ignore = "unable to pass block proof"]
    fn test_may_have_unfinalized_no_prev_block() {
        unreachable!()
    }

    #[test]
    fn test_may_have_unfinalized_prev_wrong_block() {
        let test_case = sample_case();

        let last_withdrawal_index = {
            let withdrawals = test_case.builder.withdrawals.get(&1).unwrap();
            withdrawals.len().saturating_sub(1) as u32
        };

        // Exclude block #1
        let mut test_case = test_case
            .into_builder()
            .prev_last_finalized_withdrawal(1, BLOCK_ALL_WITHDRAWALS)
            .build();

        let err_prev_index = LastFinalizedWithdrawal::new_builder()
            .block_number(1.pack())
            .withdrawal_index(last_withdrawal_index.pack())
            .build();

        test_case.prev_global_state = test_case
            .prev_global_state
            .as_builder()
            .last_finalized_withdrawal(err_prev_index)
            .build();

        expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
    }

    #[test]
    fn test_may_have_unfinalized_prev_invalid_index() {
        let test_case = sample_case();

        // umcomparable prev index against WithdrawalIndex::NoWithdrawal
        {
            let mut test_case = test_case
                .clone()
                .into_builder()
                .prev_last_finalized_withdrawal(3, BLOCK_ALL_WITHDRAWALS)
                .build();

            // add block #3
            let block_smt = test_case.block_withdrawals.block_smt();
            let block_proof = block_smt
                .merkle_proof(vec![H256::from_u64(3), H256::from_u64(4)])
                .unwrap()
                .compile(vec![
                    (H256::from_u64(3), H256::zero()),
                    (H256::from_u64(4), H256::zero()),
                ])
                .unwrap();

            let block_withdrawals = RawL2BlockWithdrawals::new_builder()
                .raw_l2block(test_case.block_withdrawals.blocks.get(3).unwrap().raw())
                .build();

            let mut block_withdrawals_vec = vec![block_withdrawals];
            block_withdrawals_vec.extend(
                test_case
                    .finalize_withdrawal
                    .block_withdrawals()
                    .into_iter(),
            );

            test_case.finalize_withdrawal = RollupFinalizeWithdrawal::new_builder()
                .block_proof(block_proof.0.pack())
                .block_withdrawals(
                    RawL2BlockWithdrawalsVec::new_builder()
                        .set(block_withdrawals_vec)
                        .build(),
                )
                .build();

            let err_prev_index = LastFinalizedWithdrawal::new_builder()
                .block_number(3.pack())
                .withdrawal_index(0u32.pack())
                .build();

            test_case.prev_global_state = test_case
                .prev_global_state
                .as_builder()
                .last_finalized_withdrawal(err_prev_index)
                .build();

            expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
        }

        // prev index > last withdrawal index
        {
            let mut test_case = test_case;
            let last_withdrawal_index = {
                let withdrawals = test_case.builder.withdrawals.get(&1).unwrap();
                withdrawals.len().saturating_sub(1) as u32
            };

            let err_prev_index = LastFinalizedWithdrawal::new_builder()
                .block_number(1.pack())
                .withdrawal_index((last_withdrawal_index + 1).pack())
                .build();

            test_case.prev_global_state = test_case
                .prev_global_state
                .as_builder()
                .last_finalized_withdrawal(err_prev_index)
                .build();

            expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
        }
    }

    #[test]
    fn test_post_invalid_index() {
        let test_case = sample_case();

        // uncomparable post index against WithdrawalIndex::NoWithdrawal
        {
            let mut test_case = test_case
                .clone()
                .into_builder()
                .post_last_finalized_withdrawal(3, BLOCK_ALL_WITHDRAWALS)
                .build();

            let err_post_index = LastFinalizedWithdrawal::new_builder()
                .block_number(3.pack())
                .withdrawal_index(0u32.pack())
                .build();

            test_case.post_global_state = test_case
                .post_global_state
                .as_builder()
                .last_finalized_withdrawal(err_post_index)
                .build();

            expect_err!(test_case, ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL);
        }

        // post index > last withdrawal index
        {
            let mut test_case = test_case
                .into_builder()
                .post_last_finalized_withdrawal(2, BLOCK_ALL_WITHDRAWALS)
                .build();

            let last_withdrawal_index = {
                let withdrawals = test_case.builder.withdrawals.get(&2).unwrap();
                withdrawals.len().saturating_sub(1) as u32
            };

            let err_post_index = LastFinalizedWithdrawal::new_builder()
                .block_number(2.pack())
                .withdrawal_index((last_withdrawal_index + 1).pack())
                .build();

            test_case.post_global_state = test_case
                .post_global_state
                .as_builder()
                .last_finalized_withdrawal(err_post_index)
                .build();

            expect_err!(test_case, ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL);
        }
    }

    #[test]
    fn test_witness_blocks_len_dont_match_index_range() {
        let mut test_case = sample_case()
            .into_builder()
            .prev_last_finalized_withdrawal(0, BLOCK_ALL_WITHDRAWALS)
            .post_last_finalized_withdrawal(2, BLOCK_ALL_WITHDRAWALS)
            .build();

        let err_post_index = LastFinalizedWithdrawal::new_builder()
            .block_number(4.pack())
            .withdrawal_index(BLOCK_ALL_WITHDRAWALS.pack())
            .build();

        test_case.post_global_state = test_case
            .post_global_state
            .as_builder()
            .last_finalized_withdrawal(err_post_index)
            .build();

        expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
    }

    #[test]
    fn test_witness_blocks_not_in_ascending_order() {
        let mut test_case = sample_case();

        let mut err_block_withdrawals_vec = test_case
            .finalize_withdrawal
            .block_withdrawals()
            .into_iter()
            .collect::<Vec<_>>();

        // Swap second and third block
        err_block_withdrawals_vec.swap(1, 2);

        test_case.finalize_withdrawal = test_case
            .finalize_withdrawal
            .as_builder()
            .block_withdrawals(
                RawL2BlockWithdrawalsVec::new_builder()
                    .set(err_block_withdrawals_vec)
                    .build(),
            )
            .build();

        // witnes wrong block
        expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
    }
}

mod check_inclusive_range_withdrawals {
    use super::*;
    use gw_types::packed::{
        CKBMerkleProof, RawL2BlockWithdrawals, RawL2BlockWithdrawalsVec, RollupFinalizeWithdrawal,
    };

    fn sample_case() -> TestCase {
        TestCase::builder()
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .push_withdrawal(1, 1000 * CKB, 100)
            .last_finalized_block(1)
            .prev_last_finalized_withdrawal(1, 0)
            .post_last_finalized_withdrawal(1, 2)
            .build()
    }

    #[test]
    #[ignore = "impossible, checked before enter logic"]
    fn test_start_index_is_greater_than_end_index() {
        unreachable!()
    }

    #[test]
    fn test_invalid_index() {
        let test_case = sample_case();

        // start > last withdrawal index
        {
            let mut test_case = test_case.clone();

            let err_prev_last_finalized_withdrawal = test_case
                .prev_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(99u32.pack())
                .build();

            test_case.prev_global_state = test_case
                .prev_global_state
                .as_builder()
                .last_finalized_withdrawal(err_prev_last_finalized_withdrawal)
                .build();

            let err_post_last_finalized_withdrawal = test_case
                .post_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(100u32.pack())
                .build();

            test_case.post_global_state = test_case
                .post_global_state
                .as_builder()
                .last_finalized_withdrawal(err_post_last_finalized_withdrawal)
                .build();

            expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
        }

        // end > last withdrawal index
        {
            let mut test_case = test_case;

            let err_post_last_finalized_withdrawal = test_case
                .post_global_state
                .last_finalized_withdrawal()
                .as_builder()
                .withdrawal_index(100u32.pack())
                .build();

            test_case.post_global_state = test_case
                .post_global_state
                .as_builder()
                .last_finalized_withdrawal(err_post_last_finalized_withdrawal)
                .build();

            expect_err!(test_case, ERROR_INVALID_LAST_FINALIZED_WITHDRAWAL);
        }
    }

    // Trigger `0 == withdrawal_count`
    #[test]
    fn test_witness_submit_block_without_withdrawal() {
        let mut test_case = sample_case()
            .into_builder()
            .push_empty_block(2)
            .push_empty_block(2 + FINALITY_BLOCKS) // update last finalized block to 2
            .build();

        // Replace use block #2, which is empty block
        test_case.finalize_withdrawal = {
            let block_smt = test_case.block_withdrawals.block_smt();
            let block_proof = block_smt
                .merkle_proof(vec![H256::from_u64(2)])
                .unwrap()
                .compile(vec![(H256::from_u64(2), H256::zero())])
                .unwrap();

            let block_withdrawals = RawL2BlockWithdrawals::new_builder()
                .raw_l2block(test_case.block_withdrawals.blocks.get(1).unwrap().raw())
                .build();

            RollupFinalizeWithdrawal::new_builder()
                .block_proof(block_proof.0.pack())
                .block_withdrawals(
                    RawL2BlockWithdrawalsVec::new_builder()
                        .set(vec![block_withdrawals])
                        .build(),
                )
                .build()
        };

        // Replace dummy index
        let prev = LastFinalizedWithdrawal::new_builder()
            .block_number(2.pack())
            .withdrawal_index(0u32.pack())
            .build();

        test_case.prev_global_state = test_case
            .prev_global_state
            .as_builder()
            .last_finalized_withdrawal(prev)
            .build();

        let post = LastFinalizedWithdrawal::new_builder()
            .block_number(2.pack())
            .withdrawal_index(1u32.pack())
            .build();

        test_case.post_global_state = test_case
            .post_global_state
            .as_builder()
            .last_finalized_withdrawal(post)
            .build();

        expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
    }

    #[test]
    fn test_witness_submit_withdrawals_not_match_index_range() {
        let test_case = sample_case()
            .into_builder()
            .prev_last_finalized_withdrawal(1, 0)
            .post_last_finalized_withdrawal(1, 2)
            .build();

        // Submit extra withdrawals
        {
            let mut test_case = test_case.clone();

            test_case.finalize_withdrawal = {
                let prev = LastFinalizedWithdrawal::new_builder()
                    .block_number(1.pack())
                    .withdrawal_index(0u32.pack())
                    .build();

                let post = LastFinalizedWithdrawal::new_builder()
                    .block_number(1.pack())
                    .withdrawal_index(4u32.pack())
                    .build();

                let (_, finalize_withdrawal) = test_case
                    .block_withdrawals
                    .generate_finalize_withdrawals(&prev, &post);

                finalize_withdrawal
            };

            expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
        }

        // Skip withdrawals
        {
            let mut test_case = test_case;

            test_case.finalize_withdrawal = {
                let prev = LastFinalizedWithdrawal::new_builder()
                    .block_number(1.pack())
                    .withdrawal_index(0u32.pack())
                    .build();

                let post = LastFinalizedWithdrawal::new_builder()
                    .block_number(1.pack())
                    .withdrawal_index(1u32.pack())
                    .build();

                let (_, finalize_withdrawal) = test_case
                    .block_withdrawals
                    .generate_finalize_withdrawals(&prev, &post);

                finalize_withdrawal
            };

            expect_err!(test_case, ERROR_INVALID_ROLLUP_FINALIZE_WITHDRAWAL_WITNESS);
        }
    }

    #[test]
    fn test_witness_invalid_withdrawal_merkle_proof() {
        let mut test_case = sample_case();

        test_case.finalize_withdrawal = {
            let block_withdrawals = test_case
                .finalize_withdrawal
                .block_withdrawals()
                .get(0)
                .unwrap();

            let err_block_withdrawals = block_withdrawals
                .as_builder()
                .withdrawal_proof(CKBMerkleProof::default())
                .build();

            test_case
                .finalize_withdrawal
                .as_builder()
                .block_withdrawals(
                    RawL2BlockWithdrawalsVec::new_builder()
                        .set(vec![err_block_withdrawals])
                        .build(),
                )
                .build()
        };

        expect_err!(test_case, ERROR_MERKLE_PROOF);
    }
}
