use ckb_types::packed::{Bytes, CellDep, CellInput, CellOutput, OutPoint, Script, WitnessArgs};
use ckb_types::prelude::{Builder, Entity, Pack};

pub trait ToCKBType<T> {
    fn to_ckb(&self) -> T;
}

macro_rules! impl_to_ckb {
    ($type_:tt) => {
        impl ToCKBType<$type_> for gw_types::packed::$type_ {
            fn to_ckb(&self) -> $type_ {
                $type_::new_unchecked(self.as_bytes())
            }
        }
    };
}
impl_to_ckb!(Script);
impl_to_ckb!(CellInput);
impl_to_ckb!(CellOutput);
impl_to_ckb!(WitnessArgs);
impl_to_ckb!(CellDep);

impl ToCKBType<Bytes> for gw_types::bytes::Bytes {
    fn to_ckb(&self) -> Bytes {
        self.pack()
    }
}

impl ToCKBType<(CellOutput, Bytes)> for (gw_types::packed::CellOutput, gw_types::bytes::Bytes) {
    fn to_ckb(&self) -> (CellOutput, Bytes) {
        (self.0.to_ckb(), self.1.to_ckb())
    }
}

pub trait ToGWType<T> {
    fn to_gw(&self) -> T;
}

macro_rules! impl_to_gw {
    ($type_:tt) => {
        impl ToGWType<gw_types::packed::$type_> for $type_ {
            fn to_gw(&self) -> gw_types::packed::$type_ {
                gw_types::packed::$type_::new_unchecked(self.as_bytes())
            }
        }
    };
}

impl_to_gw!(OutPoint);
impl_to_gw!(CellOutput);
impl_to_gw!(Script);

pub trait CKBTypeIntoExt<T> {
    fn into_ext(self) -> T;
}

impl CKBTypeIntoExt<CellInput> for OutPoint {
    fn into_ext(self) -> CellInput {
        CellInput::new_builder().previous_output(self).build()
    }
}

impl CKBTypeIntoExt<CellDep> for OutPoint {
    fn into_ext(self) -> CellDep {
        CellDep::new_builder().out_point(self).build()
    }
}
