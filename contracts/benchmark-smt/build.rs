use gw_types::{packed::*, prelude::*};
use std::env;
use std::path::Path;
use std::fs::{File, self};
use std::io::{BufWriter, Write, Read};

fn main() {
    // parse debug tx json
    let mut f = File::open("../../debug.txt").unwrap();
    let mut content = String::new();
    f.read_to_string(&mut content).unwrap();
    let content = hex::decode(&content.trim().trim_start_matches("0x")).unwrap();
    let witness_args = WitnessArgs::from_slice(&content).unwrap();
    let witness = witness_args.output_type().to_opt().unwrap();
    let raw_witness: Vec<u8> = witness.unpack();
    let action = RollupAction::from_slice(&raw_witness).unwrap();
    let args = match action.to_enum() {
        RollupActionUnion::RollupSubmitBlock(args) => args,
        _ => {
            panic!("unknown action")
        }
    };
    let l2block = args.block();

    // extract kv pairs & proof
    // write to source code

    // let out_path = Path::new(&env::var("OUT_DIR").unwrap()).join("raw_data.rs");
    let out_path = Path::new(&"src").join("raw_data.rs");
    let mut out_file = BufWriter::new(File::create(&out_path).expect("create raw_data.rs"));

    writeln!(
        &mut out_file,
        "pub const BIN_BLOCK: [u8; {}] = {:?};",
        l2block.as_slice().len(),
        l2block.as_slice(),
    )
    .expect("write to raw_data.rs");
}
