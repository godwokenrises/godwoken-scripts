/*
 * SUDT Utils
 * Godwoken backend use this utils to modify SUDT states from the SMT.
 */

#include "godwoken.h"
#include "gw_def.h"
#include "stdio.h"

/* errors */
#define ERROR_INSUFFICIENT_BALANCE 12
#define ERROR_AMOUNT_OVERFLOW 13
#define ERROR_TO_ADDR 14
#define ERROR_ACCOUNT_NOT_EXISTS 15
#define ERROR_SHORT_ADDR_LEN 16

/* Prepare withdrawal fields */
#define WITHDRAWAL_LOCK_HASH 1
#define WITHDRAWAL_AMOUNT 2
#define WITHDRAWAL_BLOCK_NUMBER 3

int _sudt_emit_log(gw_context_t *ctx,
                   const uint32_t sudt_id,
                   const uint64_t short_addr_len,
                   const uint8_t *from_addr,
                   const uint8_t *to_addr,
                   const uint128_t amount,
                   uint8_t service_flag) {
#ifdef GW_VALIDATOR
  uint32_t data_size = 0;
  uint8_t *data = NULL;
#else
  uint32_t data_size = 1 + short_addr_len * 2 + 16;
  uint8_t data[128] = {0};
  data[0] = (uint8_t)short_addr_len;
  memcpy(data + 1, from_addr, short_addr_len);
  memcpy(data + 1 + short_addr_len, to_addr, short_addr_len);
  memcpy(data + 1 + short_addr_len * 2, (uint8_t *)(&amount), 16);
#endif
  return ctx->sys_log(ctx, sudt_id, service_flag, data_size, data);
}

int _sudt_get_balance(gw_context_t *ctx, uint32_t sudt_id,
                      const uint8_t *key,
                      const uint64_t key_len,
                      uint128_t *balance) {
  uint8_t value[32] = {0};
  int ret = ctx->sys_load(ctx, sudt_id, key, key_len, value);
  if (ret != 0) {
    return ret;
  }
  *balance = *(uint128_t *)value;
  return 0;
}

int _sudt_set_balance(gw_context_t *ctx, uint32_t sudt_id,
                      const uint8_t *key,
                      const uint64_t key_len,
                      uint128_t balance) {
  uint8_t value[32] = {0};
  *(uint128_t *)value = balance;
  int ret = ctx->sys_store(ctx, sudt_id, key, key_len, value);
  return ret;
}

int sudt_get_balance(gw_context_t *ctx,
                     const uint32_t sudt_id,
                     const uint64_t short_addr_len,
                     const uint8_t *short_address, uint128_t *balance) {
  if (short_addr_len > 32) {
    return ERROR_SHORT_ADDR_LEN;
  }
  int ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }
  return _sudt_get_balance(ctx, sudt_id, short_address, short_addr_len, balance);
}

/* Transfer Simple UDT */
int _sudt_transfer(gw_context_t *ctx,
                   const uint32_t sudt_id,
                   const uint64_t short_addr_len,
                   const uint8_t *from_addr,
                   const uint8_t *to_addr,
                   const uint128_t amount,
                   uint8_t service_flag) {
  int ret;
  if (memcmp(from_addr, to_addr, short_addr_len) == 0) {
    return ERROR_TO_ADDR;
  }
  ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }

  /* check from account */
  uint128_t from_balance;
  ret = _sudt_get_balance(ctx, sudt_id, from_addr, short_addr_len, &from_balance);
  if (ret != 0) {
    return ret;
  }
  if (from_balance < amount) {
    return ERROR_INSUFFICIENT_BALANCE;
  }
  uint128_t new_from_balance = from_balance - amount;

  /* check to account */
  uint128_t to_balance;
  ret = _sudt_get_balance(ctx, sudt_id, to_addr, short_addr_len, &to_balance);
  if (ret != 0) {
    return ret;
  }
  uint128_t new_to_balance = to_balance + amount;
  if (new_to_balance < to_balance) {
    return ERROR_AMOUNT_OVERFLOW;
  }

  /* update balance */
  ret = _sudt_set_balance(ctx, sudt_id, from_addr, short_addr_len, new_from_balance);
  if (ret != 0) {
    return ret;
  }
  ret = _sudt_set_balance(ctx, sudt_id, to_addr, short_addr_len, new_to_balance);
  if (ret != 0) {
    return ret;
  }
  return _sudt_emit_log(ctx, sudt_id, short_addr_len, from_addr, to_addr, amount, service_flag);
}

int sudt_transfer(gw_context_t *ctx,
                  const uint32_t sudt_id,
                  const uint64_t short_addr_len,
                  const uint8_t *from_addr,
                  const uint8_t *to_addr,
                  const uint128_t amount) {
  if (short_addr_len > 32) {
    return ERROR_SHORT_ADDR_LEN;
  }
  return _sudt_transfer(ctx, sudt_id, short_addr_len, from_addr, to_addr, amount, GW_LOG_SUDT_TRANSFER);
}

/* Pay fee */
int sudt_pay_fee(gw_context_t *ctx,
                 const uint32_t sudt_id,
                 const uint64_t short_addr_len,
                 const uint8_t *from_addr,
                 const uint128_t amount) {
  if (short_addr_len > 32) {
    return ERROR_SHORT_ADDR_LEN;
  }
  uint32_t to_id = ctx->block_info.block_producer_id;
  /* The script hash's pointer also it's prefix's pointer */
  uint8_t to_script_hash[32] = {0};
  int ret = ctx->sys_get_script_hash_by_account_id(ctx, to_id, to_script_hash);
  if (ret != 0) {
    return ret;
  }
  return _sudt_transfer(ctx, sudt_id, short_addr_len, from_addr, to_script_hash, amount, GW_LOG_SUDT_PAY_FEE);
}
