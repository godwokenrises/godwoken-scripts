use core::{ops::RangeInclusive, result::Result};

use gw_utils::{
    ckb_std::debug,
    error::Error,
    gw_types::{core::WithdrawalCursorIndex, packed::RawL2BlockReader, prelude::Unpack},
};

#[derive(Clone, Debug)]
pub enum WithdrawalIndexRange {
    All,
    RangeInclusive(RangeInclusive<u32>),
}

impl WithdrawalIndexRange {
    pub fn build_range_inclusive(
        raw_block: &RawL2BlockReader,
        start: WithdrawalCursorIndex,
        end: WithdrawalCursorIndex,
    ) -> Result<Self, Error> {
        let count = raw_block.submit_withdrawals().withdrawal_count().unpack();
        match (start, end) {
            (WithdrawalCursorIndex::All, WithdrawalCursorIndex::Index(_)) => {
                debug!("WithdrawalIndexRange: invalid range");
                Err(Error::FinalizeWithdrawal)
            }
            (WithdrawalCursorIndex::All, WithdrawalCursorIndex::All) => Ok(Self::All),
            (WithdrawalCursorIndex::Index(start), WithdrawalCursorIndex::All) if start == 0 => {
                Ok(Self::All)
            }
            (WithdrawalCursorIndex::Index(start), WithdrawalCursorIndex::All) => {
                if start >= count {
                    debug!(
                        "WithdrawalIndexRange: invalid range index {:?} is out of count {:?}",
                        start, count
                    );
                    return Err(Error::FinalizeWithdrawal);
                }
                let range = RangeInclusive::new(start, count - 1);
                Ok(WithdrawalIndexRange::RangeInclusive(range))
            }
            (WithdrawalCursorIndex::Index(start), WithdrawalCursorIndex::Index(end)) => {
                if start > end {
                    debug!(
                        "WithdrawalIndexRange: invalid range {:?} >= {:?}",
                        start, end
                    );
                    return Err(Error::FinalizeWithdrawal);
                }

                if end >= count {
                    debug!(
                        "WithdrawalIndexRange: invalid range index {:?} is out of count {:?}",
                        end, count
                    );
                    return Err(Error::FinalizeWithdrawal);
                }

                if end + 1 == count {
                    debug!("WithdrawalIndexRange: must use All to present the last element");
                    return Err(Error::FinalizeWithdrawal);
                }

                let range = RangeInclusive::new(start, end);
                Ok(WithdrawalIndexRange::RangeInclusive(range))
            }
        }
    }
}
