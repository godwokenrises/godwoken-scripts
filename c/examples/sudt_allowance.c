/*
 * This code is for testing sudt_utils.h
 *
 * `sudt_transfer` and `sudt_get_balance` used in c/contracts/sudt.c and tested
 * in tests/src/script_tests/l2_scripts/sudt.rs. So this file is just test `sudt_set_allowance` and `sudt_get_allowance`
 */

#include "ckb_syscalls.h"
#include "gw_syscalls.h"
#include "stdio.h"
#include "sudt_utils.h"

#define FLAG_SET_ALLOWANCE 0xf1
#define FLAG_GET_ALLOWANCE 0xf2

int main() {
  gw_context_t ctx = {0};
  int ret = gw_context_init(&ctx);
  if (ret != 0) {
    return ret;
  }

  if (ctx.transaction_context.args_len < 1) {
    return -1;
  }
  uint32_t args_len = ctx.transaction_context.args_len;
  uint8_t *content = ctx.transaction_context.args + 1;
  uint8_t flag = ctx.transaction_context.args[0];
  /* args_len must >=29bytes or >=13bytes */
  if (flag == FLAG_SET_ALLOWANCE) {
    if (args_len < 29) {
      ckb_debug("invalid length for set allowance");
      return -1;
    }
    uint32_t sudt_id = *((uint32_t *)content);
    /* NOTE: read owner_id from args is just for tests, in backend code owner_id must be trusted value */
    uint32_t owner_id = *((uint32_t *)(content + 4));
    uint32_t spender_id = *((uint32_t *)(content + 8));
    uint128_t amount = *((uint128_t *)(content + 12));
    return sudt_set_allowance(&ctx, sudt_id, owner_id, spender_id, amount);
  } else if (flag == FLAG_GET_ALLOWANCE) {
    if (args_len < 13) {
      ckb_debug("invalid length for get allowance");
      return -1;
    }
    uint32_t sudt_id = *((uint32_t *)content);
    uint32_t owner_id = *((uint32_t *)(content + 4));
    uint32_t spender_id = *((uint32_t *)(content + 8));
    uint128_t amount = 0;
    ret = sudt_get_allowance(&ctx, sudt_id, owner_id, spender_id, &amount);
    if (ret != 0) {
      ckb_debug("sudt_get_allowance failed");
      return ret;
    }
    return ctx.sys_set_program_return_data(&ctx, (uint8_t *)&amount, sizeof(amount));
  } else {
    ckb_debug("invalid flag");
    return -1;
  }
}
