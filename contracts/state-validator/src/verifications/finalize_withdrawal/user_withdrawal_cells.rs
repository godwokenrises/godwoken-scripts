use core::result::Result;

use alloc::collections::BTreeMap;
use gw_utils::{
    cells::{
        lock_cells::{collect_custodian_locks, collect_user_withdrawal_cells},
        types::UserWithdrawalCell,
        utils::build_assets_map_from_cells,
    },
    ckb_std::{ckb_constants::Source, debug},
    error::Error,
    gw_common::{CKB_SUDT_SCRIPT_ARGS, H256},
    gw_types::{
        packed::{RawL2BlockWithdrawalsVecReader, RollupConfig},
        prelude::{Pack, Unpack},
    },
};

#[must_use]
pub fn check(
    rollup_type_hash: &H256,
    rollup_config: &RollupConfig,
    last_finalized_block_number: u64,
    block_withdrawals_vec: &RawL2BlockWithdrawalsVecReader,
) -> Result<(), Error> {
    debug!("check user withdrawal cells");

    let input_finalized_assets = collect_finalized_assets(
        rollup_type_hash,
        last_finalized_block_number,
        rollup_config,
        Source::Input,
    )?;
    let withdrawal_request_assets = build_withdrawal_request_assets(block_withdrawals_vec)?;

    let remained_input_finalized_assets = sub_balance_from_output_user_withdrawal_cells(
        rollup_config,
        input_finalized_assets,
        withdrawal_request_assets,
    )?;

    let output_finalized_assets = collect_finalized_assets(
        rollup_type_hash,
        last_finalized_block_number,
        rollup_config,
        Source::Output,
    )?;

    if remained_input_finalized_assets != output_finalized_assets {
        debug!("wrong output custodian balance");
        debug!("remained input asset {:?}", remained_input_finalized_assets);
        debug!("output custodian asset {:?}", output_finalized_assets);
        return Err(Error::InvalidUserWithdrawalCell);
    }

    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct FinalizedAssetsMap(BTreeMap<H256, u128>);

impl FinalizedAssetsMap {
    fn has_balance(&self) -> bool {
        !self.0.is_empty()
    }

    fn sub_balance_from_withdrawal_cell(&mut self, cell: &UserWithdrawalCell) -> Result<(), Error> {
        self.sub_withdrawal_amount(&CKB_SUDT_SCRIPT_ARGS.into(), cell.value.capacity.into())?;
        if 0 != cell.value.amount {
            self.sub_withdrawal_amount(&cell.value.sudt_script_hash, cell.value.amount)?;
        }

        Ok(())
    }

    fn sub_withdrawal_amount(&mut self, sudt_type_hash: &H256, amount: u128) -> Result<(), Error> {
        let sudt_balance_mut = match self.0.get_mut(sudt_type_hash) {
            Some(balance) => balance,
            None => {
                debug!("unknown withdrawal sudt {:x}", sudt_type_hash.pack());
                return Err(Error::InvalidUserWithdrawalCell);
            }
        };
        match sudt_balance_mut.checked_sub(amount) {
            Some(balance) => *sudt_balance_mut = balance,
            None => {
                debug!("withdraw balance overflow");
                return Err(Error::AmountOverflow);
            }
        }
        if 0 == *sudt_balance_mut {
            drop(sudt_balance_mut);
            self.0.remove(sudt_type_hash);
        }

        Ok(())
    }
}

#[must_use]
fn collect_finalized_assets(
    rollup_type_hash: &H256,
    last_finalized_block_number: u64,
    config: &RollupConfig,
    source: Source,
) -> Result<FinalizedAssetsMap, Error> {
    debug!("collect finalized assets from source {:?}", source);

    let custodian_cells = collect_custodian_locks(rollup_type_hash, config, source)?;
    let has_unfinalized_custodian_cell = custodian_cells.iter().any(|cell| {
        let deposit_block_number = cell.args.deposit_block_number().unpack();
        deposit_block_number > last_finalized_block_number
    });
    if has_unfinalized_custodian_cell {
        debug!("custodian cells contain unfinalized one");
        return Err(Error::InvalidCustodianCell);
    }

    let assets = build_assets_map_from_cells(custodian_cells.iter().map(|c| &c.value))?;
    Ok(FinalizedAssetsMap(assets))
}

#[must_use]
fn build_withdrawal_request_assets(
    block_withdrawals_vec: &RawL2BlockWithdrawalsVecReader,
) -> Result<BTreeMap<H256, FinalizedAssetsMap>, Error> {
    debug!("build user withdrawal assets");

    let ckb_sudt_type_hash: H256 = CKB_SUDT_SCRIPT_ARGS.into();
    let mut user_assets = BTreeMap::new();
    for block_withdrawals in block_withdrawals_vec.iter() {
        for withdrawal in block_withdrawals.withdrawals().iter() {
            let raw_req = withdrawal.raw();
            let lock_hash = raw_req.owner_lock_hash().unpack();

            let assets_mut = user_assets
                .entry(lock_hash)
                .or_insert(FinalizedAssetsMap(BTreeMap::new()));

            let ckb_amount = raw_req.capacity().unpack();
            let ckb_balance_mut = assets_mut.0.entry(ckb_sudt_type_hash).or_insert(0u128);
            *ckb_balance_mut = ckb_balance_mut
                .checked_add(ckb_amount.into())
                .ok_or(Error::AmountOverflow)?;

            let sudt_amount = raw_req.amount().unpack();
            let sudt_type_hash: H256 = raw_req.sudt_script_hash().unpack();
            if 0 != sudt_amount && ckb_sudt_type_hash != sudt_type_hash {
                let sudt_balance_mut = assets_mut.0.entry(sudt_type_hash).or_insert(0u128);
                *sudt_balance_mut = sudt_balance_mut
                    .checked_add(sudt_amount)
                    .ok_or(Error::AmountOverflow)?;
            }
        }
    }

    Ok(user_assets)
}

#[must_use]
fn sub_balance_from_output_user_withdrawal_cells(
    rollup_config: &RollupConfig,
    mut input_finalized_assets: FinalizedAssetsMap,
    withdrawal_request_assets: BTreeMap<H256, FinalizedAssetsMap>,
) -> Result<FinalizedAssetsMap, Error> {
    for (lock_hash, mut request_assets) in withdrawal_request_assets.into_iter() {
        debug!("check withdrawal from lock hash {:x}", lock_hash.pack());

        let user_withdrawal_cells = collect_user_withdrawal_cells(rollup_config, &lock_hash)?;
        for cell in user_withdrawal_cells {
            debug!("check output idx {} user withdrawal cell", cell.index);

            request_assets.sub_balance_from_withdrawal_cell(&cell)?;
            input_finalized_assets.sub_balance_from_withdrawal_cell(&cell)?;
        }

        if request_assets.has_balance() {
            debug!("unfullfilled withdrawal request");
            return Err(Error::InvalidUserWithdrawalCell);
        }
    }

    Ok(input_finalized_assets)
}
