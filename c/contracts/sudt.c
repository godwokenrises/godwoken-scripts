/*
 * SUDT compatible layer2 contract
 * This contract is designed as the SUDT equivalent contract on layer2.
 *
 * One layer2 SUDT contract is mapping to one layer1 SUDT contract
 *
 * We use the sudt_script_hash of SUDT cells in layer2 script args to
 * destinguish different SUDT tokens, which described in the RFC:
 * https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0025-simple-udt/0025-simple-udt.md#sudt-cell
 *
 * Basic APIs to supports transfer token:
 *
 * * query(account_id) -> balance
 * * transfer(to, amount, fee)
 *
 * # Mint & Burn
 *
 * To join a Rollup, users deposit SUDT assets on layer1;
 * then Rollup aggregators take the layer1 assets and mint new SUDT coins on
 * layer2 according to the deposited assets.
 * (Aggregator find a corresponded layer2 SUDT contract by searching
 * sudt_script_hash, or create one if the SUDT hasn't been deposited before)
 *
 * To leave a Rollup, the Rollup aggregators burn SUDT coins from layer2 and
 * send the layer1 SUDT assets to users.
 *
 * The aggregators operate Mint & Burn by directly modify the state tree.
 */

#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "stdio.h"
#include "sudt_utils.h"

/* MSG_TYPE */
#define MSG_QUERY 0
#define MSG_TRANSFER 1

int main() {
  /* initialize context */
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  };

  /* parse SUDT args */
  mol_seg_t args_seg;
  args_seg.ptr = ctx.transaction_context.args;
  args_seg.size = ctx.transaction_context.args_len;
  if (MolReader_SUDTArgs_verify(&args_seg, false) != MOL_OK) {
    return GW_ERROR_INVALID_DATA;
  }
  mol_union_t msg = MolReader_SUDTArgs_unpack(&args_seg);
  uint32_t sudt_id = ctx.transaction_context.to_id;

  /* Handle messages */
  if (msg.item_id == MSG_QUERY) {
    /* Query */
    mol_seg_t short_address_seg = MolReader_SUDTQuery_get_short_address(&msg.seg);
    uint64_t short_addr_len = (uint64_t)MolReader_Bytes_length(&short_address_seg);
    mol_seg_t raw_short_address_seg = MolReader_Bytes_raw_bytes(&short_address_seg);
    uint128_t balance = 0;
    ret = sudt_get_balance(&ctx, sudt_id, short_addr_len, raw_short_address_seg.ptr, &balance);
    if (ret != 0) {
      return ret;
    }
    ret = ctx.sys_set_program_return_data(&ctx, (uint8_t *)&balance,
                                          sizeof(uint128_t));
    if (ret != 0) {
      return ret;
    }
  } else if (msg.item_id == MSG_TRANSFER) {
    /* Transfer */
    mol_seg_t to_seg = MolReader_SUDTTransfer_get_to(&msg.seg);
    uint64_t short_addr_len = (uint64_t)MolReader_Bytes_length(&to_seg);
    mol_seg_t raw_to_seg = MolReader_Bytes_raw_bytes(&to_seg);

    mol_seg_t amount_seg = MolReader_SUDTTransfer_get_amount(&msg.seg);
    mol_seg_t fee_seg = MolReader_SUDTTransfer_get_fee(&msg.seg);
    uint32_t from_id = ctx.transaction_context.from_id;
    uint8_t from_script_hash[32] = {0};
    ret = ctx.sys_get_script_hash_by_account_id(&ctx, from_id, from_script_hash);
    if (ret != 0) {
      return ret;
    }
    /* The prefix */
    uint8_t *from_addr = from_script_hash;
    uint8_t *to_addr = raw_to_seg.ptr;

    uint128_t amount = *(uint128_t *)amount_seg.ptr;
    uint128_t fee = *(uint128_t *)fee_seg.ptr;
    /* pay fee */
    ret = sudt_pay_fee(&ctx, sudt_id, short_addr_len, from_addr, fee);
    if (ret != 0) {
      ckb_debug("pay fee failed");
      return ret;
    }
    /* transfer */
    ret = sudt_transfer(&ctx, sudt_id, short_addr_len, from_addr, to_addr, amount);
    if (ret != 0) {
      ckb_debug("transfer token failed");
      return ret;
    }
    /* set return data, from addr balance | block producer balance | to addr balance */
    uint8_t return_data[48] = {0};
    uint128_t balance = 0;

    uint32_t block_producer_id = ctx.block_info.block_producer_id;
    uint8_t block_producer_script_hash[32] = {0};
    ret = ctx.sys_get_script_hash_by_account_id(&ctx, block_producer_id, block_producer_script_hash);
    if (ret != 0) {
        ckb_debug("can't find block producer id");
        return ret;
    }

    ret = sudt_get_balance(&ctx, sudt_id, short_addr_len, from_addr, &balance);
    if (ret != 0) {
        ckb_debug("get from balance failed");
        return ret;
    }
    memcpy(return_data, (uint8_t *)(&balance), 16);

    ret = sudt_get_balance(&ctx, sudt_id, short_addr_len, block_producer_script_hash, &balance);
    if (ret != 0) {
        ckb_debug("get block producer balance failed");
        return ret;
    }
    memcpy(return_data + 16, (uint8_t *)(&balance), 16);

    ret = sudt_get_balance(&ctx, sudt_id, short_addr_len, to_addr, &balance);
    if (ret != 0) {
        ckb_debug("get to balance failed");
        return ret;
    }
    memcpy(return_data + 32, (uint8_t *)(&balance), 16);

    ret = ctx.sys_set_program_return_data(&ctx, (uint8_t *)&return_data, 48);
    if (ret != 0) {
        ckb_debug("set return data failed");
        return ret;
    }
  } else {
    return GW_ERROR_UNKNOWN_ARGS;
  }

  return gw_finalize(&ctx);
}
