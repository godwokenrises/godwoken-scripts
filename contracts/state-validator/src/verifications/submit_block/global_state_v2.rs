use gw_utils::{
    ckb_std::debug,
    error::Error,
    gw_types::{
        packed::{GlobalState, LastFinalizedWithdrawal, RawL2BlockReader},
        prelude::{Builder, Entity, Pack, Unpack},
    },
};

use crate::verifications::finalize_withdrawal::last_finalized_withdrawal::LAST_FINALIZED_WITHDRAWAL_INDEX_ALL_WITHDRAWALS;

pub struct GlobalStateV2Verifications {
    pub check_no_output_withdrawal_cells: bool,
    pub check_prev_last_finalized_withdrawal_field_is_default: bool,
}

// NOTE: The only place that modify `last_finalized_withdrawal` in submit block is upgrade to
// v2 process, so we don't need to check prev last finalized withdrawal field is default when
// post global state is v2.
impl GlobalStateV2Verifications {
    pub fn from_post_global_state(post_global_state: &GlobalState) -> Result<Self, Error> {
        let verifications = match post_global_state.version_u8() {
            0 | 1 => GlobalStateV2Verifications {
                check_no_output_withdrawal_cells: false,
                check_prev_last_finalized_withdrawal_field_is_default: true,
            },
            2 => GlobalStateV2Verifications {
                check_no_output_withdrawal_cells: true,
                check_prev_last_finalized_withdrawal_field_is_default: false,
            },
            ver => {
                debug!("invalid global state version {}", ver);
                return Err(Error::InvalidGlobalStateVersion);
            }
        };

        Ok(verifications)
    }

    pub fn can_upgrade_to_v2(
        prev_global_state: &GlobalState,
        post_global_state: &GlobalState,
    ) -> bool {
        prev_global_state.version_u8() < 2 && post_global_state.version_u8() >= 2
    }

    pub fn upgrade_to_v2(global_state: GlobalState, raw_l2block: &RawL2BlockReader) -> GlobalState {
        let parent_block_number = raw_l2block.number().unpack().saturating_sub(1);

        let last_finalized_withdrawal = LastFinalizedWithdrawal::new_builder()
            .block_number(parent_block_number.pack())
            .withdrawal_index(LAST_FINALIZED_WITHDRAWAL_INDEX_ALL_WITHDRAWALS.pack())
            .build();

        global_state
            .as_builder()
            .last_finalized_withdrawal(last_finalized_withdrawal)
            .version(2u8.into())
            .build()
    }
}
