use alloc::{collections::BTreeMap, vec::Vec};
use ckb_std::{
    ckb_constants::Source,
    high_level::{load_cell_lock_hash, QueryIter},
};
use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{RollupConfig, Script},
    prelude::*,
};

use crate::{cells::types::CellValue, error::Error};

pub fn search_lock_hashes(owner_lock_hash: &[u8; 32], source: Source) -> Vec<usize> {
    QueryIter::new(load_cell_lock_hash, source)
        .enumerate()
        .filter_map(|(i, lock_hash)| {
            if &lock_hash == owner_lock_hash {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

pub fn search_lock_hash(owner_lock_hash: &[u8; 32], source: Source) -> Option<usize> {
    QueryIter::new(load_cell_lock_hash, source).position(|lock_hash| &lock_hash == owner_lock_hash)
}

pub fn build_l2_sudt_script(
    rollup_script_hash: &H256,
    config: &RollupConfig,
    l1_sudt_script_hash: &H256,
) -> Script {
    let args = {
        let mut args = Vec::with_capacity(64);
        args.extend(rollup_script_hash.as_slice());
        args.extend(l1_sudt_script_hash.as_slice());
        Bytes::from(args)
    };
    Script::new_builder()
        .args(args.pack())
        .code_hash(config.l2_sudt_validator_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .build()
}

pub fn build_assets_map_from_cells<'a, I: Iterator<Item = &'a CellValue>>(
    cells: I,
) -> Result<BTreeMap<H256, u128>, Error> {
    let mut assets = BTreeMap::new();
    for cell in cells {
        let sudt_balance = assets.entry(cell.sudt_script_hash).or_insert(0u128);
        *sudt_balance = sudt_balance
            .checked_add(cell.amount)
            .ok_or(Error::AmountOverflow)?;
        let ckb_balance = assets.entry(CKB_SUDT_SCRIPT_ARGS.into()).or_insert(0u128);
        *ckb_balance = ckb_balance
            .checked_add(cell.capacity.into())
            .ok_or(Error::AmountOverflow)?;
    }
    Ok(assets)
}
