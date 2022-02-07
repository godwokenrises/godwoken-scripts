/*
 * Meta contract
 * This contract is builtin in the Godwoken Rollup, and the account_id is zero.
 *
 * We use Meta contract to implement some special features like create a
 * contract account.
 */

#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "sudt_utils.h"

/* MSG_TYPE */
#define MSG_CREATE_ACCOUNT 0
/* Currently, we only support 20 length short script hash length */
#define DEFAULT_SHORT_SCRIPT_HASH_LEN 20

int handle_fee(gw_context_t *ctx, mol_seg_t fee_seg) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* payer's short script hash */
  uint8_t payer_short_script_hash[32] = {0};
  int ret = ctx->sys_get_script_hash_by_account_id(
      ctx, ctx->transaction_context.from_id, payer_short_script_hash);
  if (ret != 0) {
    return ret;
  }
  uint64_t short_script_hash_len = DEFAULT_SHORT_SCRIPT_HASH_LEN;
  /* sudt */
  mol_seg_t sudt_id_seg = MolReader_Fee_get_sudt_id(&fee_seg);
  uint32_t sudt_id = *(uint32_t *)sudt_id_seg.ptr;
  /* amount */
  mol_seg_t amount_seg = MolReader_Fee_get_amount(&fee_seg);
  uint128_t amount = *(uint128_t *)amount_seg.ptr;
  return sudt_pay_fee(ctx, sudt_id, short_script_hash_len,
                      payer_short_script_hash, amount);
}

int main() {
  /* initialize context */
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  };

  /* return error if contract account id isn't zero */
  if (ctx.transaction_context.to_id != 0) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* parse Meta contract args */
  mol_seg_t args_seg;
  args_seg.ptr = ctx.transaction_context.args;
  args_seg.size = ctx.transaction_context.args_len;
  if (MolReader_MetaContractArgs_verify(&args_seg, false) != MOL_OK) {
    return GW_FATAL_INVALID_DATA;
  }
  mol_union_t msg = MolReader_MetaContractArgs_unpack(&args_seg);

  /* Handle messages */
  if (msg.item_id == MSG_CREATE_ACCOUNT) {
    /* Charge fee */
    mol_seg_t fee_seg = MolReader_CreateAccount_get_fee(&msg.seg);
    int ret = handle_fee(&ctx, fee_seg);
    if (ret != 0) {
      return ret;
    }
    /* Create account */
    mol_seg_t script_seg = MolReader_CreateAccount_get_script(&msg.seg);
    uint32_t account_id = 0;
    ret = ctx.sys_create(&ctx, script_seg.ptr, script_seg.size, &account_id);
    if (ret != 0) {
      return ret;
    }
    ret = ctx.sys_set_program_return_data(&ctx, (uint8_t *)&account_id,
                                          sizeof(uint32_t));
    if (ret != 0) {
      return ret;
    }
  } else {
    return GW_FATAL_UNKNOWN_ARGS;
  }
  return gw_finalize(&ctx);
}
