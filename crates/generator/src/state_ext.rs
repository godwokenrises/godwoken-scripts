use crate::generator::DepositionRequest;
use crate::syscalls::RunResult;
use gw_common::state::{Error, State, ZERO};

pub trait StateExt {
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error>;
    fn apply_deposition_requests(
        &mut self,
        deposition_requests: &[DepositionRequest],
    ) -> Result<(), Error>;
}

impl<S: State> StateExt for S {
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error> {
        for (k, v) in &run_result.write_values {
            self.update_raw((*k).into(), (*v).into())?;
        }
        Ok(())
    }

    fn apply_deposition_requests(
        &mut self,
        deposition_requests: &[DepositionRequest],
    ) -> Result<(), Error> {
        for request in deposition_requests {
            if request.account_id == 0 {
                let id = self.create_account(ZERO, request.pubkey_hash)?;
                self.mint_sudt(&request.token_id, id, request.value)?;
            } else {
                self.mint_sudt(&request.token_id, request.account_id, request.value)?;
            }
        }

        Ok(())
    }
}