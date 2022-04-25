#include "godwoken.h"
#include "gw_def.h"
#include "gw_registry_addr.h"
#include "gw_syscalls.h"
#include "sudt_utils.h"
#include "uint256.h"

/* format:
 * from_addr | to_addr | amount(32 bytes)
 */
int _u256_sudt_emit_log(gw_context_t *ctx, const uint32_t sudt_id,
                        gw_reg_addr_t from_addr, gw_reg_addr_t to_addr,
                        const uint256_t amount, uint8_t service_flag) {
#ifdef GW_VALIDATOR
  uint32_t data_size = 0;
  uint8_t *data = NULL;
#else
  uint8_t data[256] = {0};
  /* from_addr + to_addr + amount(32 bytes) */
  uint32_t data_size =
      GW_REG_ADDR_SIZE(from_addr) + GW_REG_ADDR_SIZE(to_addr) + 32;
  if (data_size > 256) {
    printf("_u256_sudt_emit_log: data is large than buffer");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  _gw_cpy_addr(data, from_addr);
  _gw_cpy_addr(data + GW_REG_ADDR_SIZE(from_addr), to_addr);
  memcpy(data + GW_REG_ADDR_SIZE(from_addr) + GW_REG_ADDR_SIZE(to_addr),
         (uint8_t *)(&amount), 32);
#endif
  return ctx->sys_log(ctx, sudt_id, service_flag, data_size, data);
}

int _u256_sudt_get_balance(gw_context_t *ctx, const uint32_t sudt_id,
                           gw_reg_addr_t address, uint256_t *balance) {
  uint8_t key[64] = {0};
  uint32_t key_len = 64;
  int ret = _sudt_build_key(SUDT_KEY_FLAG_BALANCE, address, key, &key_len);
  if (ret != 0) {
    return ret;
  }
  uint8_t value[32] = {0};
  ret = ctx->sys_load(ctx, sudt_id, key, key_len, value);
  if (ret != 0) {
    return ret;
  }
  uint256_from_little_endian((uint8_t *)&value, 32, balance);
  return 0;
}

int _u256_sudt_set_balance(gw_context_t *ctx, const uint32_t sudt_id,
                           gw_reg_addr_t address, uint256_t balance) {
  uint8_t key[64] = {0};
  uint32_t key_len = 64;
  int ret = _sudt_build_key(SUDT_KEY_FLAG_BALANCE, address, key, &key_len);
  if (ret != 0) {
    return ret;
  }

  uint8_t value[32] = {0};
  uint256_to_little_endian(balance, (uint8_t *)&value, 32);
  ret = ctx->sys_store(ctx, CKB_SUDT_ACCOUNT_ID, key, key_len, value);
  return ret;
}

int u256_sudt_get_balance(gw_context_t *ctx, const uint32_t sudt_id,
                          gw_reg_addr_t addr, uint256_t *balance) {
  int ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }
  return _u256_sudt_get_balance(ctx, sudt_id, addr, balance);
}

int ckb_get_balance(gw_context_t *ctx, gw_reg_addr_t addr, uint256_t *balance) {
  return _u256_sudt_get_balance(ctx, CKB_SUDT_ACCOUNT_ID, addr, balance);
}

int _u256_sudt_get_total_supply(gw_context_t *ctx, const uint32_t sudt_id,
                                uint256_t *total_supply) {
  uint8_t value[32] = {0};
  int ret = ctx->sys_load(ctx, sudt_id, SUDT_TOTAL_SUPPLY_KEY, 32, value);
  if (ret != 0) {
    return ret;
  }
  uint256_from_little_endian((uint8_t *)&value, 32, total_supply);
  return 0;
}

int u256_sudt_get_total_supply(gw_context_t *ctx, const uint32_t sudt_id,
                               uint256_t *total_supply) {
  int ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }
  return _u256_sudt_get_total_supply(ctx, sudt_id, total_supply);
}

int ckb_get_total_supply(gw_context_t *ctx, uint256_t *total_supply) {
  return _u256_sudt_get_total_supply(ctx, CKB_SUDT_ACCOUNT_ID, total_supply);
}

int _u256_sudt_transfer(gw_context_t *ctx, const uint32_t sudt_id,
                        gw_reg_addr_t from_addr, gw_reg_addr_t to_addr,
                        const uint256_t amount, uint8_t service_flag) {
  int ret;

  /* check from account */
  uint256_t from_balance = {0};
  ret = _u256_sudt_get_balance(ctx, sudt_id, from_addr, &from_balance);
  if (ret != 0) {
    printf("transfer: can't get sender's balance");
    return ret;
  }
  if (uint256_cmp(from_balance, amount) == SMALLER) {
    printf("transfer: insufficient balance");
    return GW_SUDT_ERROR_INSUFFICIENT_BALANCE;
  }

  if (_gw_cmp_addr(from_addr, to_addr) == 0) {
    printf("transfer: [warning] transfer to self");
  }

  uint256_t new_from_balance = {0};
  uint256_underflow_sub(from_balance, amount, &new_from_balance);

  /* update sender balance */
  ret = _u256_sudt_set_balance(ctx, sudt_id, from_addr, new_from_balance);
  if (ret != 0) {
    printf("transfer: update sender's balance failed");
    return ret;
  }

  /* check to account */
  uint256_t to_balance = {0};
  ret = _u256_sudt_get_balance(ctx, sudt_id, to_addr, &to_balance);
  if (ret != 0) {
    printf("transfer: can't get receiver's balance");
    return ret;
  }

  uint256_t new_to_balance = {0};
  int overflow = uint256_overflow_add(to_balance, amount, &new_to_balance);
  if (overflow) {
    printf("transfer: balance overflow");
    return GW_SUDT_ERROR_AMOUNT_OVERFLOW;
  }

  /* update receiver balance */
  ret = _u256_sudt_set_balance(ctx, sudt_id, to_addr, new_to_balance);
  if (ret != 0) {
    printf("transfer: update receiver's balance failed");
    return ret;
  }

  /* emit log */
  ret = _u256_sudt_emit_log(ctx, sudt_id, from_addr, to_addr, amount,
                            service_flag);
  if (ret != 0) {
    printf("transfer: emit log failed");
  }
  return ret;
}

int u256_sudt_transfer(gw_context_t *ctx, const uint32_t sudt_id,
                       gw_reg_addr_t from_addr, gw_reg_addr_t to_addr,
                       const uint256_t amount) {
  int ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }
  return _u256_sudt_transfer(ctx, sudt_id, from_addr, to_addr, amount,
                             GW_LOG_SUDT_TRANSFER);
}

int ckb_transfer(gw_context_t *ctx, gw_reg_addr_t from_addr,
                 gw_reg_addr_t to_addr, const uint256_t amount) {
  return _u256_sudt_transfer(ctx, CKB_SUDT_ACCOUNT_ID, from_addr, to_addr,
                             amount, GW_LOG_SUDT_TRANSFER);
}

int u256_sudt_pay_fee(gw_context_t *ctx, const uint32_t sudt_id,
                      gw_reg_addr_t from_addr, const uint256_t amount) {
  int ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }

  ret = _u256_sudt_transfer(ctx, sudt_id, from_addr,
                            ctx->block_info.block_producer, amount,
                            GW_LOG_SUDT_PAY_FEE);
  if (ret != 0) {
    printf("pay fee transfer failed");
    return ret;
  }

  /* call syscall, we use this action to emit event to runtime, this function
  do
   * not actually pay the fee */
  ret = ctx->sys_pay_fee(ctx, from_addr, sudt_id, amount);
  if (ret != 0) {
    printf("sys pay fee failed");
  }
  return ret;
}

/* Pay fee */
int ckb_pay_fee(gw_context_t *ctx, gw_reg_addr_t from_addr,
                const uint256_t amount) {
  int ret = _u256_sudt_transfer(ctx, CKB_SUDT_ACCOUNT_ID, from_addr,
                                ctx->block_info.block_producer, amount,
                                GW_LOG_SUDT_PAY_FEE);
  if (ret != 0) {
    printf("pay fee transfer failed");
    return ret;
  }

  /* call syscall, we use this action to emit event to runtime, this function
  do
   * not actually pay the fee */
  ret = ctx->sys_pay_fee(ctx, from_addr, CKB_SUDT_ACCOUNT_ID, amount);
  if (ret != 0) {
    printf("sys pay fee failed");
  }
  return ret;
}
