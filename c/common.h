#ifndef GW_COMMON_H_
#define GW_COMMON_H_

#include "blake2b.h"
#include "blockchain.h"
#include "ckb_smt.h"
#include "godwoken.h"
#include "gw_def.h"
#include "gw_errors.h"
#include "stddef.h"

/* common functions,
   This file should be included in the generator_utils.h & validator_utils.h
    after the gw_context_t structure is defined.
 */

/* Implement of gw_blake2b_hash_fn
 * Note: this function is used in layer2 contract
 */
void blake2b_hash(uint8_t output_hash[GW_KEY_BYTES], uint8_t *input_data,
                  uint32_t len) {
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  blake2b_update(&blake2b_ctx, input_data, len);
  blake2b_final(&blake2b_ctx, output_hash, GW_KEY_BYTES);
}

/* Generate raw key
 * raw_key: blake2b(id | type | key)
 *
 * We use raw key in the underlying KV store
 */
void gw_build_account_key(uint32_t id, const uint8_t *key, const size_t key_len,
                          uint8_t raw_key[GW_KEY_BYTES]) {
  uint8_t type = GW_ACCOUNT_KV;
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  blake2b_update(&blake2b_ctx, (uint8_t *)&id, 4);
  blake2b_update(&blake2b_ctx, (uint8_t *)&type, 1);
  blake2b_update(&blake2b_ctx, key, key_len);
  blake2b_final(&blake2b_ctx, raw_key, GW_KEY_BYTES);
}

void gw_build_account_field_key(uint32_t id, uint8_t field_type,
                                uint8_t key[GW_KEY_BYTES]) {
  memset(key, 0, 32);
  memcpy(key, (uint8_t *)(&id), sizeof(uint32_t));
  key[sizeof(uint32_t)] = field_type;
}

void gw_build_script_hash_to_account_id_key(uint8_t script_hash[GW_KEY_BYTES],
                                            uint8_t raw_key[GW_KEY_BYTES]) {
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  uint32_t placeholder = 0;
  blake2b_update(&blake2b_ctx, (uint8_t *)&placeholder, 4);
  uint8_t type = GW_ACCOUNT_SCRIPT_HASH_TO_ID;
  blake2b_update(&blake2b_ctx, (uint8_t *)&type, 1);
  blake2b_update(&blake2b_ctx, script_hash, GW_KEY_BYTES);
  blake2b_final(&blake2b_ctx, raw_key, GW_KEY_BYTES);
}

void gw_build_data_hash_key(uint8_t data_hash[GW_KEY_BYTES],
                            uint8_t raw_key[GW_KEY_BYTES]) {
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  uint32_t placeholder = 0;
  blake2b_update(&blake2b_ctx, (uint8_t *)&placeholder, 4);
  uint8_t type = GW_DATA_HASH_PREFIX;
  blake2b_update(&blake2b_ctx, (uint8_t *)&type, 1);
  blake2b_update(&blake2b_ctx, data_hash, GW_KEY_BYTES);
  blake2b_final(&blake2b_ctx, raw_key, GW_KEY_BYTES);
}

int gw_build_short_script_hash_to_script_hash_key(
    uint8_t *short_script_hash, uint32_t short_script_hash_len,
    uint8_t raw_key[GW_KEY_BYTES]) {
  if (short_script_hash_len > 32 || short_script_hash == NULL) {
    return GW_FATAL_INVALID_DATA;
  }

  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);

  /* placeholder: 0 */
  uint32_t placeholder = 0;
  blake2b_update(&blake2b_ctx, (uint8_t *)&placeholder, 4);
  /* type */
  uint8_t type = GW_SHORT_ACCOUNT_SCRIPT_HASH_TO_SCRIPT_HASH;
  blake2b_update(&blake2b_ctx, (uint8_t *)&type, 1);
  /* short_script_hash_len */
  blake2b_update(&blake2b_ctx, (uint8_t *)&short_script_hash_len,
                 sizeof(uint32_t));
  /* short_script_hash */
  blake2b_update(&blake2b_ctx, short_script_hash, short_script_hash_len);

  blake2b_final(&blake2b_ctx, raw_key, GW_KEY_BYTES);
  return 0;
}

int gw_parse_transaction_context(gw_transaction_context_t *transaction_context,
                                 mol_seg_t *src) {
  if (MolReader_RawL2Transaction_verify(src, false) != MOL_OK) {
    return GW_FATAL_INVALID_DATA;
  }
  mol_seg_t from_id_seg = MolReader_RawL2Transaction_get_from_id(src);
  mol_seg_t to_id_seg = MolReader_RawL2Transaction_get_to_id(src);
  mol_seg_t args_bytes_seg = MolReader_RawL2Transaction_get_args(src);
  mol_seg_t args_seg = MolReader_Bytes_raw_bytes(&args_bytes_seg);
  if (args_seg.size > GW_MAX_L2TX_ARGS_SIZE) {
    return GW_FATAL_INVALID_DATA;
  }
  transaction_context->from_id = *(uint32_t *)from_id_seg.ptr;
  transaction_context->to_id = *(uint32_t *)to_id_seg.ptr;
  memcpy(transaction_context->args, args_seg.ptr, args_seg.size);
  transaction_context->args_len = args_seg.size;
  return 0;
}

int gw_parse_block_info(gw_block_info_t *block_info, mol_seg_t *src) {
  if (MolReader_BlockInfo_verify(src, false) != MOL_OK) {
    return GW_FATAL_INVALID_DATA;
  }
  mol_seg_t number_seg = MolReader_BlockInfo_get_number(src);
  mol_seg_t timestamp_seg = MolReader_BlockInfo_get_timestamp(src);
  mol_seg_t block_producer_id_seg =
      MolReader_BlockInfo_get_block_producer_id(src);
  memcpy(&block_info->number, number_seg.ptr, sizeof(uint64_t));
  memcpy(&block_info->timestamp, timestamp_seg.ptr, sizeof(uint64_t));
  block_info->block_producer_id = *(uint32_t *)block_producer_id_seg.ptr;
  return 0;
}

/* check zero hash */
int _is_zero_hash(const uint8_t hash[32]) { return _smt_zero_value(hash); }

/* ensure account id is exist */
int _ensure_account_exists(gw_context_t *ctx, uint32_t account_id) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  uint8_t script_hash[32];
  uint8_t raw_key[32] = {0};
  gw_build_account_field_key(account_id, GW_ACCOUNT_SCRIPT_HASH, raw_key);
  int ret = ctx->_internal_load_raw(ctx, raw_key, script_hash);
  if (ret != 0) {
    return ret;
  }

  if (_is_zero_hash(script_hash)) {
    return GW_ERROR_ACCOUNT_NOT_EXISTS;
  }

  return 0;
}

/* ensure account script hash is exist */
int _check_account_exists_by_script_hash(gw_context_t *ctx,
                                         uint8_t script_hash[32],
                                         int *is_exist) {
  /* Compare with meta contract */
  uint8_t meta_script_hash[32] = {0};
  uint8_t raw_key[32] = {0};
  gw_build_account_field_key(0, GW_ACCOUNT_SCRIPT_HASH, raw_key);
  int ret = ctx->_internal_load_raw(ctx, raw_key, meta_script_hash);
  if (ret != 0) {
    return ret;
  }
  if (memcmp(meta_script_hash, script_hash, 32) == 0) {
    *is_exist = true;
    return 0;
  }

  /* check script_hash to account_id */
  uint8_t value[32] = {0};
  gw_build_script_hash_to_account_id_key(script_hash, raw_key);
  ret = ctx->_internal_load_raw(ctx, raw_key, value);
  if (ret != 0) {
    return ret;
  }
  uint32_t account_id = 0;
  memcpy((uint8_t *)&account_id, value, sizeof(uint32_t));

  *is_exist = account_id != 0;
  return 0;
}

int _load_sender_nonce(gw_context_t *ctx, uint32_t *sender_nonce) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  /* sender nonce */
  uint8_t nonce_key[32] = {0};
  uint8_t nonce_value[32] = {0};

  gw_build_account_field_key(ctx->transaction_context.from_id, GW_ACCOUNT_NONCE,
                             nonce_key);
  int ret = ctx->_internal_load_raw(ctx, nonce_key, nonce_value);
  if (ret != 0) {
    printf("failed to fetch sender nonce value");
    return ret;
  }
  memcpy(sender_nonce, nonce_value, sizeof(uint32_t));
  return 0;
}

int _increase_sender_nonce(gw_context_t *ctx) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  /* load sender nonce */
  uint32_t new_nonce = 0;
  int ret = _load_sender_nonce(ctx, &new_nonce);
  if (ret != 0) {
    return ret;
  }
  if (new_nonce < ctx->original_sender_nonce) {
    printf("sender's new_nonce is less than original_nonce");
    return GW_FATAL_INVALID_CONTEXT;
  } else if (new_nonce == ctx->original_sender_nonce) {
    printf("new_nonce is equals to original_nonce, increase 1");
    new_nonce += 1;
    uint8_t nonce_key[32] = {0};
    uint8_t nonce_value[32] = {0};
    /* prepare key value */
    gw_build_account_field_key(ctx->transaction_context.from_id,
                               GW_ACCOUNT_NONCE, nonce_key);
    memcpy(nonce_value, (uint8_t *)&new_nonce, sizeof(uint32_t));

    ret = ctx->_internal_store_raw(ctx, nonce_key, nonce_value);
    if (ret != 0) {
      printf("failed to update sender nonce value");
      return ret;
    }
  }

  return 0;
}

int _check_data_hash_exist(gw_context_t *ctx, uint8_t data_hash[32],
                           int *is_exist) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  /* Check data_hash_key */
  uint8_t raw_key[GW_KEY_BYTES] = {0};
  uint8_t data_exists[GW_VALUE_BYTES] = {0};
  gw_build_data_hash_key(data_hash, raw_key);
  int ret = ctx->_internal_load_raw(ctx, raw_key, data_exists);
  if (ret != 0) {
    return ret;
  }

  *is_exist = !_is_zero_hash(data_exists);
  return 0;
}

int _load_script_hash_by_short_script_hash(gw_context_t *ctx,
                                           uint8_t *short_script_hash,
                                           uint32_t short_script_hash_len,
                                           uint8_t script_hash[32]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  if (short_script_hash == NULL) {
    return GW_FATAL_INVALID_DATA;
  }
  /* Check short_script_hash_key */
  uint8_t raw_key[GW_KEY_BYTES] = {0};
  int ret = gw_build_short_script_hash_to_script_hash_key(
      short_script_hash, short_script_hash_len, raw_key);
  if (ret != 0) {
    return ret;
  }
  ret = ctx->_internal_load_raw(ctx, raw_key, script_hash);
  if (ret != 0) {
    return ret;
  }

  if (_is_zero_hash(script_hash)) {
    return GW_ERROR_NOT_FOUND;
  }

  return 0;
}

#endif /* GW_COMMON_H_ */
