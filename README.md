# Godwoken-scripts

On-chain scripts of [Godwoken](https://github.com/nervosnetwork/godwoken) project.

## Directory layout

``` txt
root
├─ c: Layer-2 built-in C scripts
│  ├─ contracts/meta_contract.c: The Meta contract operating layer-2 accounts
│  ├─ contracts/eth_addr_reg.c: Mapping Ethereum address to Godwoken account
│  ├─ contracts/sudt.c: The layer-2 Simple UDT contract
│  ├─ contracts/examples: Example contracts
├─ c-uint256-tests: tests of uint256 C implementation
├─ contracts: Layer-1 Godwoken scripts
│  ├─ always-success: A script always returns true, used in tests
│  ├─ challenge-lock: The lock script checks setup of a challenge
│  ├─ ckb-smt: SMT no-std implementation
│  ├─ custodian-lock: The lock script protects custodian cells
│  ├─ deposit-lock: The lock script of user deposits
│  ├─ eth-account-lock: The lock script used to check Ethereum signatures on-chain
│  ├─ gw-state: Godwoken state tree implementation
│  ├─ gw-utils: Common functions used in Godwoken scripts
│  ├─ secp256k1-utils: Secp256k1
│  ├─ stake-lock: The lock script of stake cell
│  ├─ state-validator: The type script constaint the on-chain operation of Rollup cell
│  ├─ tron-account-lock: The lock script used to check Tron signatures on-chain(deprecated)
│  ├─ withdrawal-lock: The lock script protects withdrawal cells
├─ tests: scripting tests
├─ tools: tools used in CI
```
