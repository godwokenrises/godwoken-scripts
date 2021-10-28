/**
 * `ETH Address Registry` layer2 contract
 * 
 * This contract introduces two-ways mappings between `eth_address` and
 * `gw_script_hash`.
 *   - As the rightmost 160 bits of a Keccak hash of an ECDSA public key,
 *     `eth_address` represents an EOA or contract address on Ethereum.
 *   - Godwoken account script hash(a.k.a. `gw_script_hash`) is a key used for
 *     locating the account lock. Note that Godwoken enforces one-to-one mapping
 *     between layer 2 lock script and account ID.
 * 
 * There are 2 kinds of accounts in Godwoken: 
 *   1) Typical user accounts denoted by an account lock
 *   2) Contract accounts denoted by a backend script
 */

#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "stdio.h"

/* MSG_TYPE */
#define MSG_QUERY_ETH_TO_GW 0
#define MSG_QUERY_GW_TO_ETH 1

int main() {
  /* initialize context */
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  };

  /* verify and parse args */
  mol_seg_t args_seg;
  args_seg.ptr = ctx.transaction_context.args;
  args_seg.size = ctx.transaction_context.args_len;
  if (MolReader_ETHAddrRegArgs_verify(&args_seg, false) != MOL_OK) {
    return GW_FATAL_INVALID_DATA;
  }
  mol_union_t msg = MolReader_ETHAddrRegArgs_unpack(&args_seg);

  /* handle message */
  if (msg.item_id == MSG_QUERY_ETH_TO_GW) {
    mol_seg_t eth_address_seg = MolReader_EthToGw_get_eth_address(&msg.seg);
    /** TODO: special-case: meta-contract address */

    // assume it is a ETH EoA address, get gw_script_hash of ETH_EoA from store
    uint8_t script_hash[GW_VALUE_BYTES] = {0};
    ret = load_script_hash_by_eth_address(&ctx,
                                          eth_address_seg.ptr,
                                          script_hash);
    if (GW_ERROR_NOT_FOUND == ret) {
      // assume it is a Polyjuice contract_address
      ret = _load_script_hash_by_short_script_hash(&ctx,
                                                   eth_address_seg.ptr,
                                                   ETH_ADDRESS_LEN,
                                                   script_hash);
    }
    if (ret != 0) {
      return ret;
    }
    ret = ctx.sys_set_program_return_data(&ctx, script_hash, GW_VALUE_BYTES);
    if (ret != 0) {
      return ret;
    }
  }
  else if (msg.item_id == MSG_QUERY_GW_TO_ETH) {
    mol_seg_t script_hash_seg = MolReader_GwToEth_get_gw_script_hash(&msg.seg);

    uint8_t eth_address[ETH_ADDRESS_LEN] = {0};
    ret = load_eth_address_by_script_hash(&ctx,
                                          script_hash_seg.ptr,
                                          eth_address);
    if (ret != 0) {
      return ret;
    }
/** 
 * TODO: handle the addresses which were not inserted into ETH Address Registry
 *   is_short_script_hash_on_chain?
 *   EoA account?
 *   contract addr?
 */
    ret = ctx.sys_set_program_return_data(&ctx, eth_address, ETH_ADDRESS_LEN);
    if (ret != 0) {
      return ret;
    }
  }
  else {
    return GW_FATAL_UNKNOWN_ARGS;
  }

  return gw_finalize(&ctx);
}