//! godwoken validator errors

use ckb_std::error::SysError;
use gw_common::{error::Error as CommonError, smt::Error as SMTError};

/// Error
#[repr(i8)]
pub enum Error {
    IndexOutOfBound = 1,
    ItemMissing,
    LengthNotEnough,
    Encoding,
    // Add customized errors here...
    InvalidArgs,
    InvalidSince,
    InvalidOutput,
    OwnerCellNotFound,
    RollupCellNotFound,
    RollupConfigNotFound,
    ProofNotFound,
    AccountNotFound,
    MerkleProof,
    AmountOverflow,
    InvalidShortScriptHash,
    InsufficientAmount,
    InsufficientInputFinalizedAssets,
    InsufficientOutputFinalizedAssets,
    SMTKeyMissing,
    InvalidStateCheckpoint,
    InvalidBlock,
    InvalidStatus,
    InvalidStakeCellUnlock,
    InvalidPostGlobalState,
    InvalidChallengeCell,
    InvalidStakeCell,
    InvalidDepositCell,
    InvalidWithdrawalCell,
    InvalidCustodianCell,
    InvalidRevertedBlocks,
    InvalidChallengeReward,
    InvalidSUDTCell,
    InvalidChallengeTarget,
    InvalidWithdrawalRequest,
    UnknownEOAScript,
    UnknownContractScript,
    ScriptNotFound,
    AccountLockCellNotFound,
    AccountScriptCellNotFound,
    InvalidTypeID,
    UnexpectedTxNonce,
    // raise from signature verification script
    WrongSignature,
    DuplicatedScriptHash,
    RegistryAddressNotFound,
}

impl From<SysError> for Error {
    fn from(err: SysError) -> Self {
        use SysError::*;
        match err {
            IndexOutOfBound => Self::IndexOutOfBound,
            ItemMissing => Self::ItemMissing,
            LengthNotEnough(_) => Self::LengthNotEnough,
            Encoding => Self::Encoding,
            Unknown(err_code) => panic!("unexpected sys error {}", err_code),
        }
    }
}

impl From<CommonError> for Error {
    fn from(err: CommonError) -> Self {
        use CommonError::*;
        match err {
            SMT(_) | Store | MissingKey => Self::SMTKeyMissing,
            MerkleProof => Self::MerkleProof,
            AmountOverflow => Self::AmountOverflow,
            InvalidShortScriptHash => Self::InvalidShortScriptHash,
            DuplicatedScriptHash => Self::DuplicatedScriptHash,
        }
    }
}

impl From<SMTError> for Error {
    fn from(_err: SMTError) -> Self {
        Self::SMTKeyMissing
    }
}
