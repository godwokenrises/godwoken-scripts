use ckb_std::{
    ckb_constants::Source,
    high_level::{
        load_cell_data, load_cell_data_hash, load_cell_type_hash, load_witness_args, QueryIter,
    },
    syscalls::SysError,
};
use gw_types::{
    bytes::Bytes,
    packed::{
        GlobalState, GlobalStateReader, RollupAction, RollupActionReader, RollupConfig,
        RollupConfigReader,
    },
    prelude::*,
};

use crate::error::Error;

pub fn search_rollup_cell(rollup_type_hash: &[u8; 32], source: Source) -> Option<usize> {
    QueryIter::new(load_cell_type_hash, source)
        .position(|type_hash| type_hash.as_ref() == Some(rollup_type_hash))
}

fn search_rollup_config_cell(rollup_config_hash: &[u8; 32]) -> Option<usize> {
    QueryIter::new(load_cell_data_hash, Source::CellDep)
        .position(|data_hash| data_hash.as_ref() == rollup_config_hash)
}

pub fn load_rollup_config(rollup_config_hash: &[u8; 32]) -> Result<RollupConfig, Error> {
    let index = search_rollup_config_cell(rollup_config_hash).ok_or(Error::RollupConfigNotFound)?;
    let data = load_cell_data(index, Source::CellDep)?;
    match RollupConfigReader::verify(&data, false) {
        Ok(_) => Ok(RollupConfig::new_unchecked(data.into())),
        Err(_) => Err(Error::Encoding),
    }
}

pub fn search_rollup_state(
    rollup_type_hash: &[u8; 32],
    source: Source,
) -> Result<Option<GlobalState>, SysError> {
    let index = match QueryIter::new(load_cell_type_hash, source)
        .position(|type_hash| type_hash.as_ref() == Some(rollup_type_hash))
    {
        Some(i) => i,
        None => return Ok(None),
    };
    let data = load_cell_data(index, source)?;
    match GlobalStateReader::verify(&data, false) {
        Ok(()) => Ok(Some(GlobalState::new_unchecked(data.into()))),
        Err(_) => Err(SysError::Encoding),
    }
}

pub fn parse_rollup_action(index: usize, source: Source) -> Result<RollupAction, Error> {
    use ckb_std::ckb_types::prelude::Unpack;

    let witness_args = load_witness_args(index, source)?;
    let output_type: Bytes = witness_args
        .output_type()
        .to_opt()
        .ok_or(Error::Encoding)?
        .unpack();
    match RollupActionReader::verify(&output_type, false) {
        Ok(_) => Ok(RollupAction::new_unchecked(output_type)),
        Err(_) => Err(Error::Encoding),
    }
}
