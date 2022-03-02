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
#define CKB_SUDT_ACCOUNT_ID 1

int handle_fee(gw_context_t *ctx, uint32_t registry_id, uint64_t fee) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* payer's registry address */
  uint8_t payer_script_hash[32] = {0};
  int ret = ctx->sys_get_script_hash_by_account_id(
      ctx, ctx->transaction_context.from_id, payer_script_hash);
  if (ret != 0) {
    ckb_debug("failed to get script hash");
    return ret;
  }
  gw_reg_addr_t payer_addr;
  ret = ctx->sys_get_registry_address_by_script_hash(ctx, payer_script_hash,
                                                     registry_id, &payer_addr);
  if (ret != 0) {
    ckb_debug("failed to get payer registry address");
    return ret;
  }

  /* pay fee */
  uint32_t sudt_id = CKB_SUDT_ACCOUNT_ID;
  uint128_t fee_amount = fee;
  ret = sudt_pay_fee(ctx, sudt_id, payer_addr, fee_amount);
  if (ret != 0) {
    ckb_debug("failed to pay fee");
    return ret;
  }
  return 0;
}

int main() {
  /* initialize context */
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    ckb_debug("failed to init gw context");
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
    mol_seg_t amount_seg = MolReader_Fee_get_amount(&fee_seg);
    mol_seg_t reg_id_seg = MolReader_Fee_get_registry_id(&fee_seg);
    uint64_t fee_amount = *(uint64_t *)amount_seg.ptr;
    uint32_t reg_id = *(uint32_t *)reg_id_seg.ptr;
    int ret = handle_fee(&ctx, reg_id, fee_amount);
    if (ret != 0) {
      ckb_debug("failed to handle fee");
      return ret;
    }
    /* Create account */
    mol_seg_t script_seg = MolReader_CreateAccount_get_script(&msg.seg);
    uint32_t account_id = 0;
    ret = ctx.sys_create(&ctx, script_seg.ptr, script_seg.size, &account_id);
    if (ret != 0) {
      ckb_debug("failed to create account");
      return ret;
    }
    ret = ctx.sys_set_program_return_data(&ctx, (uint8_t *)&account_id,
                                          sizeof(uint32_t));
    if (ret != 0) {
      ckb_debug("failed to set return data");
      return ret;
    }
  } else {
    return GW_FATAL_UNKNOWN_ARGS;
  }
  return gw_finalize(&ctx);
}
