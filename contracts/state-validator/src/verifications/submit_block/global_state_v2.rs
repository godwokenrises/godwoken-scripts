use gw_utils::{
    ckb_std::debug,
    error::Error,
    gw_types::{
        packed::{GlobalState, LastFinalizedWithdrawal, RawL2BlockReader},
        prelude::{Builder, Entity, Pack, Unpack},
    },
};

use crate::verifications::finalize_withdrawal::last_finalized_withdrawal::{
    BLOCK_WITHDRAWAL_INDEX_ALL_WITHDRAWALS, BLOCK_WITHDRAWAL_INDEX_NO_WITHDRAWAL,
};

pub struct GlobalStateV2Verifications {
    pub check_no_input_reverted_withdrawal_cells: bool,
    pub check_no_output_withdrawal_cells: bool,
    pub check_last_finalized_withdrawal_field_is_default: bool,
}

impl GlobalStateV2Verifications {
    pub fn from_prev_global_state(prev_global_state: &GlobalState) -> Result<Self, Error> {
        let verifications = match prev_global_state.version_u8() {
            0 | 1 => GlobalStateV2Verifications {
                check_no_input_reverted_withdrawal_cells: false,
                check_no_output_withdrawal_cells: false,
                check_last_finalized_withdrawal_field_is_default: true,
            },
            2 => GlobalStateV2Verifications {
                check_no_input_reverted_withdrawal_cells: true,
                check_no_output_withdrawal_cells: true,
                check_last_finalized_withdrawal_field_is_default: false,
            },
            ver => {
                debug!("invalid global state version {}", ver);
                return Err(Error::InvalidGlobalStateVersion);
            }
        };

        Ok(verifications)
    }

    pub fn upgrade_to_v2(
        &self,
        global_state: GlobalState,
        raw_l2block: &RawL2BlockReader,
    ) -> GlobalState {
        let block_number = raw_l2block.number().unpack();

        let withdrawals_count: u32 = raw_l2block.submit_withdrawals().withdrawal_count().unpack();
        let withdrawal_index = if 0 == withdrawals_count {
            BLOCK_WITHDRAWAL_INDEX_NO_WITHDRAWAL
        } else {
            BLOCK_WITHDRAWAL_INDEX_ALL_WITHDRAWALS
        };

        let last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
            .block_number(block_number.pack())
            .withdrawal_index(withdrawal_index.pack())
            .build();

        global_state
            .as_builder()
            .last_finalized_withdrawal(last_finalized_withdrawal)
            .version(2u8.into())
            .build()
    }
}
