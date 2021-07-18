/*
 * SUDT Utils
 * Godwoken backend use this utils to modify SUDT states from the SMT.
 */

#include "godwoken.h"
#include "gw_def.h"
#include "overflow_add.h"
#include "stdio.h"

/* Prepare withdrawal fields */
#define WITHDRAWAL_LOCK_HASH 1
#define WITHDRAWAL_AMOUNT 2
#define WITHDRAWAL_BLOCK_NUMBER 3

#define SUDT_KEY_FLAG_BALANCE 1

void _sudt_build_key(uint32_t key_flag, const uint8_t *short_addr,
                     uint32_t short_addr_len, uint8_t *key) {
  memcpy(key, (uint8_t *)(&key_flag), 4);
  memcpy(key + 4, (uint8_t *)(&short_addr_len), 4);
  memcpy(key + 8, short_addr, short_addr_len);
}

int _sudt_emit_log(gw_context_t *ctx, const uint32_t sudt_id,
                   const uint64_t short_addr_len, const uint8_t *from_addr,
                   const uint8_t *to_addr, const uint128_t amount,
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
                      const uint8_t *short_addr, const uint64_t short_addr_len,
                      uint128_t *balance) {
  uint8_t key[32 + 8] = {0};
  uint64_t key_len = short_addr_len + 8;
  _sudt_build_key(SUDT_KEY_FLAG_BALANCE, short_addr, (uint32_t)short_addr_len,
                  key);
  uint8_t value[32] = {0};
  int ret = ctx->sys_load(ctx, sudt_id, key, key_len, value);
  if (ret != 0) {
    return ret;
  }
  *balance = *(uint128_t *)value;
  return 0;
}

int _sudt_set_balance(gw_context_t *ctx, uint32_t sudt_id,
                      const uint8_t *short_addr, const uint64_t short_addr_len,
                      uint128_t balance) {
  uint8_t key[32 + 8] = {0};
  uint64_t key_len = short_addr_len + 8;
  _sudt_build_key(SUDT_KEY_FLAG_BALANCE, short_addr, (uint32_t)short_addr_len,
                  key);

  uint8_t value[32] = {0};
  *(uint128_t *)value = balance;
  int ret = ctx->sys_store(ctx, sudt_id, key, key_len, value);
  return ret;
}

int sudt_get_balance(gw_context_t *ctx, const uint32_t sudt_id,
                     const uint64_t short_addr_len,
                     const uint8_t *short_address, uint128_t *balance) {
  if (short_addr_len > 32) {
    return GW_SUDT_ERROR_SHORT_ADDR_LEN;
  }
  int ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }
  return _sudt_get_balance(ctx, sudt_id, short_address, short_addr_len,
                           balance);
}

/* Transfer Simple UDT */
int _sudt_transfer(gw_context_t *ctx, const uint32_t sudt_id,
                   const uint64_t short_addr_len, const uint8_t *from_addr,
                   const uint8_t *to_addr, const uint128_t amount,
                   uint8_t service_flag) {
  int ret;
  ret = gw_verify_sudt_account(ctx, sudt_id);
  if (ret != 0) {
    ckb_printf("transfer: invalid sudt_id");
    return ret;
  }

  /* check from account */
  uint128_t from_balance = 0;
  ret =
      _sudt_get_balance(ctx, sudt_id, from_addr, short_addr_len, &from_balance);
  if (ret != 0) {
    ckb_printf("transfer: can't get sender's balance");
    return ret;
  }
  if (from_balance < amount) {
    ckb_printf("transfer: insufficient balance");
    return GW_SUDT_ERROR_INSUFFICIENT_BALANCE;
  }

  if (memcmp(from_addr, to_addr, short_addr_len) == 0) {
    ckb_printf("transfer: [warning] transfer to self");
  }

  uint128_t new_from_balance = from_balance - amount;

  /* update sender balance */
  ret = _sudt_set_balance(ctx, sudt_id, from_addr, short_addr_len,
                          new_from_balance);
  if (ret != 0) {
    ckb_printf("transfer: update sender's balance failed");
    return ret;
  }

  /* check to account */
  uint128_t to_balance = 0;
  ret = _sudt_get_balance(ctx, sudt_id, to_addr, short_addr_len, &to_balance);
  if (ret != 0) {
    ckb_printf("transfer: can't get receiver's balance");
    return ret;
  }

  uint128_t new_to_balance = 0;
  int overflow = uint128_overflow_add(&new_to_balance, to_balance, amount);
  if (overflow) {
    ckb_printf("transfer: balance overflow");
    return GW_SUDT_ERROR_AMOUNT_OVERFLOW;
  }

  /* update receiver balance */
  ret =
      _sudt_set_balance(ctx, sudt_id, to_addr, short_addr_len, new_to_balance);
  if (ret != 0) {
    ckb_printf("transfer: update receiver's balance failed");
    return ret;
  }

  /* emit log */
  ret = _sudt_emit_log(ctx, sudt_id, short_addr_len, from_addr, to_addr, amount,
                       service_flag);
  if (ret != 0) {
    ckb_printf("transfer: emit log failed");
  }
  return ret;
}

int sudt_transfer(gw_context_t *ctx, const uint32_t sudt_id,
                  const uint64_t short_addr_len, const uint8_t *from_addr,
                  const uint8_t *to_addr, const uint128_t amount) {
  if (short_addr_len > 32) {
    return GW_SUDT_ERROR_SHORT_ADDR_LEN;
  }
  return _sudt_transfer(ctx, sudt_id, short_addr_len, from_addr, to_addr,
                        amount, GW_LOG_SUDT_TRANSFER);
}

/* Pay fee */
int sudt_pay_fee(gw_context_t *ctx, const uint32_t sudt_id,
                 const uint64_t short_addr_len, const uint8_t *from_addr,
                 const uint128_t amount) {
  if (short_addr_len > 32) {
    ckb_printf("invalid short address len");
    return GW_SUDT_ERROR_SHORT_ADDR_LEN;
  }
  uint32_t to_id = ctx->block_info.block_producer_id;
  /* The script hash's pointer also it's prefix's pointer */
  uint8_t to_script_hash[32] = {0};
  int ret = ctx->sys_get_script_hash_by_account_id(ctx, to_id, to_script_hash);
  if (ret != 0) {
    ckb_printf("can't find to id");
    return ret;
  }
  ret = _sudt_transfer(ctx, sudt_id, short_addr_len, from_addr, to_script_hash,
                       amount, GW_LOG_SUDT_PAY_FEE);
  if (ret != 0) {
    ckb_printf("pay fee transfer failed");
    return ret;
  }
  ret = ctx->sys_pay_fee(ctx, from_addr, short_addr_len, sudt_id, amount);
  if (ret != 0) {
    ckb_printf("sys pay fee failed");
  }
  return ret;
}
