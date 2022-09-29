//! state context
//! supports read / write to global state

use gw_common::sparse_merkle_tree::H256;
use gw_utils::{ckb_std::debug, gw_common, gw_types::packed::GlobalState};

#[derive(Clone)]
pub struct DepositRequest {
    // CKB amount
    pub capacity: u64,
    // SUDT amount
    pub amount: u128,
    pub sudt_script_hash: H256,
    pub account_script_hash: H256,
}

#[derive(Clone)]
pub struct WithdrawalRequest {
    pub nonce: u32,
    // CKB amount
    pub capacity: u64,
    // SUDT amount
    pub amount: u128,
    pub sudt_script_hash: H256,
    // layer2 account_script_hash
    pub account_script_hash: H256,
    // Withdrawal request hash
    pub hash: H256,
}

pub struct BlockContext {
    pub number: u64,
    pub finalized_number: u64,
    pub timestamp: u64,
    pub block_hash: H256,
    pub rollup_type_hash: H256,
    pub prev_account_root: H256,
    pub fork_switch: ForkSwitch,
}

pub struct ForkSwitch {
    post_version: u8,
}

impl ForkSwitch {
    pub fn from_post_global_state(
        post_global_state: &GlobalState,
    ) -> Result<Self, gw_utils::error::Error> {
        let post_version = post_global_state.version_u8();
        if post_version > 2 {
            debug!("invalid global state version {}", post_version);
            return Err(gw_utils::error::Error::InvalidGlobalStateVersion);
        }

        Ok(ForkSwitch { post_version })
    }

    pub fn check_no_output_withdrawal_cells(&self) -> bool {
        self.post_version >= 2
    }

    pub fn check_withdrawal_owner_lock_in_last_witness_type_out(&self) -> bool {
        self.post_version >= 2
    }

    // NOTE: The only place that modify `last_finalized_withdrawal` in submit block is upgrade to
    // v2 process, so we don't need to check prev last finalized withdrawal field is default when
    // post global state is v2.
    pub fn check_prev_last_finalized_withdrawal_field_is_default(&self) -> bool {
        self.post_version <= 1
    }
}
