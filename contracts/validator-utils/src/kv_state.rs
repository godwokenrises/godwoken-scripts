use alloc::collections::BTreeMap;
use ckb_std::debug;
use gw_common::{error::Error, smt::Blake2bHasher, smt::CompiledMerkleProof, state::State, H256};
use gw_types::{bytes::Bytes, packed::KVPairVecReader, prelude::*};

pub struct KVState {
    kv: BTreeMap<H256, H256>,
    proof: Bytes,
    account_count: u32,
    previous_root: Option<H256>,
}

impl KVState {
    /// params:
    /// - kv_pairs, the kv pairs
    /// - proof, the merkle proof of kv_pairs
    /// - account count, account count in the current state
    /// - current_root, calculate_root returns this value if the kv_paris & proof is empty
    pub fn new(
        kv_pairs: KVPairVecReader,
        proof: Bytes,
        account_count: u32,
        current_root: Option<H256>,
    ) -> Self {
        KVState {
            kv: kv_pairs.iter().map(|kv_pair| kv_pair.unpack()).collect(),
            proof,
            account_count,
            previous_root: current_root,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.kv.is_empty() && self.proof.is_empty()
    }
}

impl State for KVState {
    fn get_raw(&self, key: &H256) -> Result<H256, Error> {
        // make sure the key must exists in the kv
        Ok(*self.kv.get(key).ok_or(Error::MissingKey)?)
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error> {
        // make sure the key must exists in the kv
        let v = self.kv.get_mut(&key).ok_or(Error::MissingKey)?;
        *v = value;
        Ok(())
    }
    fn get_account_count(&self) -> Result<u32, Error> {
        Ok(self.account_count)
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), Error> {
        self.account_count = count;
        Ok(())
    }
    fn calculate_root(&self) -> Result<H256, Error> {
        if self.is_empty() {
            return self.previous_root.ok_or_else(|| {
                debug!("calculate merkle root for an empty kv_state");
                Error::MerkleProof
            });
        }
        debug!("calculate_root: kv: {} proof: {}", self.kv.len(), self.proof.len());
        let proof = CompiledMerkleProof(self.proof.clone().into());
        let root = proof.compute_root::<Blake2bHasher>(self.kv.clone().into_iter().collect())?;
        Ok(root)
    }
}
