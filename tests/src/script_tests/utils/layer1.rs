use crate::testing_tool::programs::ALWAYS_SUCCESS_CODE_HASH;
use ckb_traits::{CellDataProvider, HeaderProvider};
use ckb_types::{
    bytes::Bytes,
    core::{
        cell::{CellMetaBuilder, ResolvedTransaction},
        EpochExt, HeaderView, ScriptHashType, TransactionView,
    },
    packed::{Byte32, CellInput, CellOutput, OutPoint, Script, Transaction},
    prelude::*,
};
use rand::{thread_rng, Rng};
use std::collections::HashMap;

pub const MAX_CYCLES: u64 = std::u64::MAX;

#[derive(Default)]
pub struct DummyDataLoader {
    pub cells: HashMap<OutPoint, (CellOutput, Bytes)>,
    pub headers: HashMap<Byte32, HeaderView>,
    pub epoches: HashMap<Byte32, EpochExt>,
}

impl CellDataProvider for DummyDataLoader {
    fn get_cell_data_hash(&self, out_point: &OutPoint) -> Option<Byte32> {
        self.cells
            .get(&out_point)
            .map(|(_, data)| CellOutput::calc_data_hash(&data))
    }

    fn get_cell_data(&self, out_point: &OutPoint) -> Option<Bytes> {
        self.cells.get(&out_point).map(|(_, data)| data.clone())
    }
}

impl HeaderProvider for DummyDataLoader {
    // load header
    fn get_header(&self, block_hash: &Byte32) -> Option<HeaderView> {
        self.headers.get(block_hash).cloned()
    }
}

pub fn always_success_script() -> Script {
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.pack())
        .hash_type(ScriptHashType::Data.into())
        .build()
}

pub fn random_out_point() -> OutPoint {
    let mut tx_hash = [0u8; 32];
    let mut rng = thread_rng();
    rng.fill(&mut tx_hash);
    OutPoint::new_builder()
        .tx_hash(tx_hash.pack())
        .index(0u32.pack())
        .build()
}

pub fn build_simple_tx(
    data_loader: &mut DummyDataLoader,
    input_cell: (CellOutput, Bytes),
    output_cell: (CellOutput, Bytes),
) -> TransactionView {
    let out_point = random_out_point();
    build_simple_tx_with_out_point(data_loader, input_cell, out_point, output_cell)
}

pub fn build_simple_tx_with_out_point(
    data_loader: &mut DummyDataLoader,
    input_cell: (CellOutput, Bytes),
    input_out_point: OutPoint,
    output_cell: (CellOutput, Bytes),
) -> TransactionView {
    data_loader
        .cells
        .insert(input_out_point.clone(), input_cell);
    let input = CellInput::new_builder()
        .previous_output(input_out_point)
        .build();
    let (output_cell, output_data) = output_cell;
    Transaction::default()
        .as_advanced_builder()
        .input(input)
        .output(output_cell)
        .output_data(output_data.pack())
        .build()
}

pub fn build_resolved_tx(
    data_loader: &DummyDataLoader,
    tx: &TransactionView,
) -> ResolvedTransaction {
    let resolved_cell_deps = tx
        .cell_deps()
        .into_iter()
        .map(|dep| {
            let deps_out_point = dep.clone();
            let (dep_output, dep_data) =
                data_loader.cells.get(&deps_out_point.out_point()).unwrap();
            CellMetaBuilder::from_cell_output(dep_output.to_owned(), dep_data.to_owned())
                .out_point(deps_out_point.out_point().clone())
                .build()
        })
        .collect();

    let mut resolved_inputs = Vec::new();
    for i in 0..tx.inputs().len() {
        let previous_out_point = tx.inputs().get(i).unwrap().previous_output();
        let (input_output, input_data) = data_loader.cells.get(&previous_out_point).unwrap();
        resolved_inputs.push(
            CellMetaBuilder::from_cell_output(input_output.to_owned(), input_data.to_owned())
                .out_point(previous_out_point)
                .build(),
        );
    }

    ResolvedTransaction {
        transaction: tx.clone(),
        resolved_cell_deps,
        resolved_inputs,
        resolved_dep_groups: vec![],
    }
}
