#ifndef GW_VALIDATOR_H_
#define GW_VALIDATOR_H_

#include "ckb_syscalls.h"
#include "gw_smt.h"
#include "gw_def.h"
#include "blockchain.h"

#define SCRIPT_HASH_TYPE_DATA 0
#define SCRIPT_HASH_TYPE_TYPE 1
#define TARGET_TYPE_TRANSACTION 0

/* buffer size */
#define GW_MAX_KV_PROOF_SIZE 4096
#define GW_MAX_CHALLENGE_LOCK_SCRIPT_SIZE 4096
#define GW_MAX_GET_BLOCK_HASH_DEPTH 256

/* functions */
int _gw_check_account_script_is_allowed(uint8_t rollup_script_hash[32],
                                        mol_seg_t *script_seg,
                                        mol_seg_t *rollup_config_seg);
void _gw_block_smt_key(uint8_t key[32], uint64_t number);

typedef struct {
  uint8_t merkle_root[32];
  uint32_t count;
} gw_account_merkle_state_t;

/* The struct is design for lazy get_account_script by account id */
typedef struct {
  uint8_t hash[32];
  uint8_t script[GW_MAX_SCRIPT_SIZE];
  uint32_t script_len;
} gw_script_entry_t;

/* Call receipt */
typedef struct {
  uint8_t return_data[GW_MAX_DATA_SIZE];
  uint32_t return_data_len;
} gw_call_receipt_t;

typedef struct gw_context_t {
  /* verification context */
  gw_transaction_context_t transaction_context;
  gw_block_info_t block_info;
  uint8_t rollup_config[GW_MAX_ROLLUP_CONFIG_SIZE];
  size_t rollup_config_size;
  uint8_t rollup_script_hash[32];

  /* layer2 syscalls */
  gw_load_fn sys_load;
  gw_store_fn sys_store;
  gw_set_program_return_data_fn sys_set_program_return_data;
  gw_create_fn sys_create;
  gw_get_account_id_by_script_hash_fn sys_get_account_id_by_script_hash;
  gw_get_script_hash_by_account_id_fn sys_get_script_hash_by_account_id;
  gw_get_account_nonce_fn sys_get_account_nonce;
  gw_get_account_script_fn sys_get_account_script;
  gw_load_data_fn sys_load_data;
  gw_store_data_fn sys_store_data;
  gw_get_block_hash_fn sys_get_block_hash;
  gw_get_script_hash_by_prefix_fn sys_get_script_hash_by_prefix;
  gw_recover_account_fn sys_recover_account;
  gw_log_fn sys_log;
  gw_pay_fee_fn sys_pay_fee;
  _gw_load_raw_fn _internal_load_raw;
  _gw_store_raw_fn _internal_store_raw;

  /* validator specific context */
  gw_account_merkle_state_t prev_account; /* RawL2Block.prev_account */
  gw_account_merkle_state_t post_account; /* RawL2Block.post_account */

  /* challenged tx index */
  uint32_t tx_index;

  /* sender's original nonce */
  uint32_t original_sender_nonce;

  /* tx check point */
  uint8_t prev_tx_checkpoint[32];
  uint8_t post_tx_checkpoint[32];

  /* kv state */
  gw_state_t kv_state;
  gw_pair_t kv_pairs[GW_MAX_KV_PAIRS];

  /* block hashes */
  gw_state_t block_hashes_state;
  gw_pair_t block_hashes_pairs[GW_MAX_GET_BLOCK_HASH_DEPTH];

  /* SMT proof */
  uint8_t kv_state_proof[GW_MAX_KV_PROOF_SIZE];
  size_t kv_state_proof_size;

  /* account count */
  uint32_t account_count;

  /* All the scripts account has read and write */
  gw_script_entry_t scripts[GW_MAX_SCRIPT_ENTRIES_SIZE];
  size_t script_entries_size;

  /* return data hash */
  uint8_t return_data_hash[32];
  gw_call_receipt_t receipt;
} gw_context_t;

#include "common.h"

int _internal_load_raw(gw_context_t *ctx,
             const uint8_t raw_key[GW_VALUE_BYTES],
             uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  return gw_state_fetch(&ctx->kv_state, raw_key, value);
}

int _internal_store_raw(gw_context_t *ctx,
              const uint8_t raw_key[GW_KEY_BYTES],
              const uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  return gw_state_insert(&ctx->kv_state, raw_key, value);
}

int sys_load(gw_context_t *ctx, uint32_t account_id, const uint8_t *key,
             const size_t key_len, uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_account_key(account_id, key, key_len, raw_key);
  return gw_state_fetch(&ctx->kv_state, raw_key, value);
}
int sys_store(gw_context_t *ctx, uint32_t account_id, const uint8_t *key,
              const size_t key_len, const uint8_t value[GW_VALUE_BYTES]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_account_key(account_id, key, key_len, raw_key);
  return gw_state_insert(&ctx->kv_state, raw_key, value);
}

/* set call return data */
int sys_set_program_return_data(gw_context_t *ctx, uint8_t *data,
                                uint64_t len) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  if (len > GW_MAX_DATA_SIZE) {
    ckb_debug("Exceeded max return data size");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  memcpy(ctx->receipt.return_data, data, len);
  ctx->receipt.return_data_len = len;
  return 0;
}

/* Get account id by account script_hash */
int sys_get_account_id_by_script_hash(gw_context_t *ctx,
                                      uint8_t script_hash[32],
                                      uint32_t *account_id) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  uint8_t raw_key[32] = {0};
  uint8_t value[32] = {0};
  gw_build_script_hash_to_account_id_key(script_hash, raw_key);
  int ret = gw_state_fetch(&ctx->kv_state, raw_key, value);
  if (ret != 0) {
    return ret;
  }
  *account_id = *((uint32_t *)value);
  return 0;
}

/* Get account script_hash by account id */
int sys_get_script_hash_by_account_id(gw_context_t *ctx, uint32_t account_id,
                                      uint8_t script_hash[32]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  uint8_t raw_key[32] = {0};
  gw_build_account_field_key(account_id, GW_ACCOUNT_SCRIPT_HASH, raw_key);
  return gw_state_fetch(&ctx->kv_state, raw_key, script_hash);
}

/* Get nonce by account id */
int sys_get_account_nonce(gw_context_t *ctx, uint32_t account_id,
                          uint32_t *nonce) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }

  uint8_t raw_key[32] = {0};
  gw_build_account_field_key(account_id, GW_ACCOUNT_NONCE, raw_key);
  uint8_t value[32] = {0};
  ret = gw_state_fetch(&ctx->kv_state, raw_key, value);
  if (ret != 0) {
    return ret;
  }
  memcpy(nonce, value, sizeof(uint32_t));
  return 0;
}

/* Get account script by account id */
int sys_get_account_script(gw_context_t *ctx, uint32_t account_id,
                           uint64_t *len, uint64_t offset, uint8_t *script) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* get account script hash */
  int ret;
  uint8_t script_hash[32] = {0};
  ret = sys_get_script_hash_by_account_id(ctx, account_id, script_hash);
  if (ret != 0) {
    return ret;
  }

  if(_is_zero_hash(script_hash)) {
    ckb_debug("account script_hash is zero, which means account isn't exist");
    return GW_ERROR_NOT_FOUND;
  }

  /* iterate all scripts to find account's script */
  gw_script_entry_t *entry = NULL;
  for (uint32_t i = 0; i < ctx->script_entries_size; i++) {
    gw_script_entry_t *current = &ctx->scripts[i];
    if (memcmp(current->hash, script_hash, 32) == 0) {
      entry = current;
      break;
    }
  }

  if (entry == NULL) {
    ckb_debug("account script_hash exist, but we can't found, we miss the neccesary context");
    return GW_FATAL_ACCOUNT_NOT_FOUND;
  }

  /* return account script */
  size_t new_len;
  size_t data_len = entry->script_len;
  if (offset >= data_len) {
    ckb_debug("account script offset is bigger than actual script len");
    new_len = 0;
  } else if ((offset + *len) > data_len) {
    new_len = data_len - offset;
  } else {
    new_len = *len;
  }
  if (new_len > 0) {
    memcpy(script, entry->script + offset, new_len);
  }
  *len = new_len;
  return 0;
}
/* Store data by data hash */
int sys_store_data(gw_context_t *ctx, uint64_t data_len, uint8_t *data) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  if (data_len > GW_MAX_DATA_SIZE) {
    ckb_debug("Exceeded max store data size");
    return GW_FATAL_INVALID_DATA;
  }
  /* In validator, we do not need to actually store data.
     We only need to update the data_hash in the state tree
   */

  /* Compute data_hash */
  uint8_t data_hash[GW_KEY_BYTES] = {0};
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, GW_KEY_BYTES);
  blake2b_update(&blake2b_ctx, data, data_len);
  blake2b_final(&blake2b_ctx, data_hash, GW_KEY_BYTES);

  /* Compute data_hash_key */
  uint8_t raw_key[GW_KEY_BYTES] = {0};
  gw_build_data_hash_key(data_hash, raw_key);

  /* value */
  uint32_t one = 1;
  uint8_t value[GW_VALUE_BYTES] = {0};
  memcpy(value, &one, sizeof(uint32_t));

  /* update state */
  return gw_state_insert(&ctx->kv_state, raw_key, value);
}

/* Load data by data hash */
int sys_load_data(gw_context_t *ctx, uint8_t data_hash[32], uint64_t *len,
                  uint64_t offset, uint8_t *data) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  int ret;
  size_t index = 0;
  uint64_t hash_len = 32;
  uint8_t hash[32] = {0};

  /* iterate all dep cells in loop */
  while (1) {
    ret = ckb_load_cell_by_field(hash, &hash_len, 0, index, CKB_SOURCE_CELL_DEP,
                                 CKB_CELL_FIELD_DATA_HASH);
    if (ret == CKB_SUCCESS) {
      /* check data hash */
      if (memcmp(hash, data_hash, 32) == 0) {
        uint64_t data_len = (uint64_t)*len;
        ret = ckb_load_cell_data(data, &data_len, offset, index,
                                 CKB_SOURCE_CELL_DEP);
        if (ret != CKB_SUCCESS) {
          ckb_debug("load cell data failed");
          return GW_FATAL_DATA_CELL_NOT_FOUND;
        }
        *len = (uint32_t)data_len;
        return 0;
      }
    } else if (ret == CKB_ITEM_MISSING) {
      ckb_debug("not found cell data by data hash");
      return GW_FATAL_DATA_CELL_NOT_FOUND;
    } else {
      ckb_debug("load cell data hash failed");
      return GW_FATAL_DATA_CELL_NOT_FOUND;
    }
    index += 1;
  }
  /* dead code */
  ckb_debug("can't find data cell");
  return GW_FATAL_INVALID_CONTEXT;
}

int sys_get_block_hash(gw_context_t *ctx, uint64_t number,
                       uint8_t block_hash[32]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  uint8_t key[32] = {0};
  _gw_block_smt_key(key, number);
  return gw_state_fetch(&ctx->block_hashes_state, key, block_hash);
}

int sys_get_script_hash_by_prefix(gw_context_t *ctx, uint8_t *prefix,
                                  uint64_t prefix_len,
                                  uint8_t script_hash[32]) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  if (prefix_len == 0 || prefix_len > 32) {
    return GW_FATAL_INVALID_DATA;
  }

  size_t i;
  for (i = 0; i < ctx->script_entries_size; i++) {
    gw_script_entry_t entry = ctx->scripts[i];
    if (memcmp(entry.hash, prefix, prefix_len) == 0) {
      memcpy(script_hash, entry.hash, 32);
      return 0;
    }
  }

  /* we don't know wether the script isn't exists or the validation context is missing */
  return GW_FATAL_INVALID_CONTEXT;
}

int sys_recover_account(gw_context_t *ctx, uint8_t message[32],
                        uint8_t *signature, uint64_t signature_len,
                        uint8_t code_hash[32], uint8_t *script,
                        uint64_t *script_len) {
  /* iterate all inputs */
  uint8_t lock_script[GW_MAX_SCRIPT_SIZE];
  uint64_t len = 0;
  uint64_t ret = 0;
  int i;
  for (i = 0; true; i++) {
    len = GW_MAX_SCRIPT_SIZE;
    /* load input's lock */
    ret = ckb_checked_load_cell_by_field(lock_script, &len, 0, i,
                                         CKB_SOURCE_INPUT, CKB_CELL_FIELD_LOCK);
    if (ret != 0) {
      return ret;
    }
    /* convert to molecule */
    mol_seg_t script_seg;
    script_seg.ptr = lock_script;
    script_seg.size = len;
    if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
      return GW_FATAL_INVALID_DATA;
    }
    /* check lock's code_hash & hash_type */
    mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
    if (memcmp(code_hash, code_hash_seg.ptr, 32) != 0) {
      continue;
    }
    mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);
    if ((*(uint8_t *)hash_type_seg.ptr) != SCRIPT_HASH_TYPE_TYPE) {
      continue;
    }
    /* load message from cell.data[32..64] */
    uint8_t checked_message[32] = {0};
    len = 32;
    ret = ckb_load_cell_data(checked_message, &len, 32, i, CKB_SOURCE_INPUT);
    if (ret != 0) {
      ckb_debug("recover account: failed to load cell data");
      continue;
    }
    if (len != 64) {
      ckb_debug("recover account: invalid data format");
      continue;
    }
    /* check message */
    if (memcmp(message, checked_message, 32) != 0) {
      continue;
    }
    /* load signature */
    uint8_t witness[GW_MAX_WITNESS_SIZE] = {0};
    len = GW_MAX_WITNESS_SIZE;
    ret = ckb_checked_load_witness(witness, &len, 0, i, CKB_SOURCE_INPUT);
    if (ret != 0) {
      ckb_debug("recover account: failed to load witness");
      continue;
    }
    mol_seg_t witness_args_seg;
    witness_args_seg.ptr = witness;
    witness_args_seg.size = len;
    if (MolReader_WitnessArgs_verify(&witness_args_seg, false) != MOL_OK) {
      ckb_debug("recover account: invalid witness args");
      continue;
    }
    mol_seg_t witness_lock_seg =
        MolReader_WitnessArgs_get_lock(&witness_args_seg);
    if (MolReader_BytesOpt_is_none(&witness_lock_seg)) {
      ckb_debug("recover account: witness args has no lock field");
      continue;
    }
    mol_seg_t signature_seg = MolReader_Bytes_raw_bytes(&witness_lock_seg);

    /* check signature */
    if (signature_len != signature_seg.size) {
      continue;
    }
    if (memcmp(signature, signature_seg.ptr, signature_len) != 0) {
      continue;
    }

    /* found script, recover account script */
    if (*script_len < script_seg.size) {
      ckb_debug("recover account: buffer overflow");
      return GW_FATAL_BUFFER_OVERFLOW;
    }
    memcpy(script, script_seg.ptr, script_seg.size);
    *script_len = script_seg.size;
    return 0;
  }
  /* Can't found account signature lock from inputs */
  ckb_debug("recover account: can't found account signature lock "
            "from inputs");
  return GW_FATAL_SIGNATURE_CELL_NOT_FOUND;
}

int sys_create(gw_context_t *ctx, uint8_t *script, uint64_t script_len,
               uint32_t *account_id) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }

  /* return failure if scripts slots is full */
  if (ctx->script_entries_size >= GW_MAX_SCRIPT_ENTRIES_SIZE) {
    ckb_debug("script slots is full");
    return GW_FATAL_BUFFER_OVERFLOW;
  }

  int ret;
  uint32_t id = ctx->account_count;

  mol_seg_t account_script_seg;
  account_script_seg.ptr = script;
  account_script_seg.size = script_len;
  /* check script */
  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = ctx->rollup_config;
  rollup_config_seg.size = ctx->rollup_config_size;
  ret = _gw_check_account_script_is_allowed(
      ctx->rollup_script_hash, &account_script_seg, &rollup_config_seg);
  if (ret != 0) {
    ckb_debug("disallowed account script");
    return ret;
  }

  /* init account nonce */
  uint8_t nonce_key[32] = {0};
  uint8_t nonce_value[32] = {0};
  gw_build_account_field_key(id, GW_ACCOUNT_NONCE, nonce_key);
  ret = gw_state_insert(&ctx->kv_state, nonce_key, nonce_value);
  if (ret != 0) {
    return ret;
  }

  /* init account script hash */
  uint8_t script_hash[32] = {0};
  uint8_t script_hash_key[32] = {0};
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, script, script_len);
  blake2b_final(&blake2b_ctx, script_hash, 32);
  gw_build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH, script_hash_key);
  ret = gw_state_insert(&ctx->kv_state, script_hash_key, script_hash);
  if (ret != 0) {
    return ret;
  }

  /* init script hash -> account_id */
  uint8_t script_hash_to_id_key[32] = {0};
  uint8_t script_hash_to_id_value[32] = {0};
  gw_build_script_hash_to_account_id_key(script_hash, script_hash_to_id_key);
  memcpy(script_hash_to_id_value, (uint8_t *)(&id), 4);
  ret = gw_state_insert(&ctx->kv_state, script_hash_to_id_key,
                        script_hash_to_id_value);
  if (ret != 0) {
    return ret;
  }

  /* build script entry */
  gw_script_entry_t script_entry = {0};
  /* copy script to entry's buf */
  memcpy(&script_entry.script, account_script_seg.ptr, account_script_seg.size);
  script_entry.script_len = account_script_seg.size;
  /* set script hash */
  memcpy(&script_entry.hash, script_hash, 32);

  /* insert script entry to ctx */
  memcpy(&ctx->scripts[ctx->script_entries_size], &script_entry,
         sizeof(gw_script_entry_t));
  ctx->script_entries_size += 1;
  ctx->account_count += 1;
  *account_id = id;

  return 0;
}

int sys_log(gw_context_t *ctx, uint32_t account_id, uint8_t service_flag,
            uint64_t data_length, const uint8_t *data) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, account_id);
  if (ret != 0) {
    return ret;
  }
  /* do nothing */
  return 0;
}

int sys_pay_fee(gw_context_t *ctx, const uint8_t *payer_addr,
                const uint64_t short_addr_len, uint32_t sudt_id,
                uint128_t amount) {
  if (ctx == NULL) {
    return GW_FATAL_INVALID_CONTEXT;
  }
  int ret = _ensure_account_exists(ctx, sudt_id);
  if (ret != 0) {
    return ret;
  }

  /* do nothing */
  return 0;
}

/* Find cell by type hash */
int _find_cell_by_type_hash(uint8_t type_hash[32], uint64_t source,
                            uint64_t *index) {
  uint8_t buf[32] = {0};
  uint64_t buf_len = 32;
  *index = 0;
  while (1) {
    int ret = ckb_checked_load_cell_by_field(buf, &buf_len, 0, *index, source,
                                             CKB_CELL_FIELD_TYPE_HASH);
    if (ret == CKB_INDEX_OUT_OF_BOUND) {
      return ret;
    }
    if (ret == CKB_SUCCESS && memcmp(type_hash, buf, 32) == 0) {
      return 0;
    }
    *index += 1;
  }
}

/* Find cell by data hash */
int _find_cell_by_data_hash(uint8_t data_hash[32], uint64_t source,
                            uint64_t *index) {
  uint8_t buf[32] = {0};
  uint64_t buf_len = 32;
  *index = 0;
  while (1) {
    int ret = ckb_checked_load_cell_by_field(buf, &buf_len, 0, *index, source,
                                             CKB_CELL_FIELD_DATA_HASH);
    if (ret == CKB_INDEX_OUT_OF_BOUND) {
      return ret;
    }
    if (ret == CKB_SUCCESS && memcmp(data_hash, buf, 32) == 0) {
      return 0;
    }
    *index += 1;
  }
}

/* load rollup script_hash from current script.args first 32 bytes */
int _load_rollup_script_hash(uint8_t rollup_script_hash[32]) {
  uint8_t script_buf[GW_MAX_SCRIPT_SIZE] = {0};
  uint64_t len = GW_MAX_SCRIPT_SIZE;
  int ret = ckb_checked_load_script(script_buf, &len, 0);
  if (ret != 0) {
    ckb_debug("failed to load script");
    return ret;
  }
  mol_seg_t script_seg;
  script_seg.ptr = script_buf;
  script_seg.size = len;
  if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
    return GW_FATAL_INVALID_DATA;
  }
  mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
  mol_seg_t raw_bytes_seg = MolReader_Bytes_raw_bytes(&args_seg);
  if (raw_bytes_seg.size < 32) {
    ckb_debug("current script is less than 32 bytes");
    return GW_FATAL_INVALID_DATA;
  }
  memcpy(rollup_script_hash, raw_bytes_seg.ptr, 32);
  return 0;
}

/* Load config config */
int _load_rollup_config(uint8_t config_cell_data_hash[32],
                        uint8_t rollup_config_buf[GW_MAX_ROLLUP_CONFIG_SIZE],
                        uint64_t *rollup_config_size) {
  /* search rollup config cell from deps */
  uint64_t config_cell_index = 0;
  int ret = _find_cell_by_data_hash(config_cell_data_hash, CKB_SOURCE_CELL_DEP,
                                    &config_cell_index);
  if (ret != 0) {
    ckb_debug("failed to find rollup config");
    return ret;
  }
  /* read data from rollup config cell */
  *rollup_config_size = GW_MAX_ROLLUP_CONFIG_SIZE;
  ret = ckb_checked_load_cell_data(rollup_config_buf, rollup_config_size, 0,
                                   config_cell_index, CKB_SOURCE_CELL_DEP);
  if (ret != 0) {
    ckb_debug("failed to load data from rollup config cell");
    return ret;
  }

  /* verify rollup config */
  mol_seg_t config_seg;
  config_seg.ptr = rollup_config_buf;
  config_seg.size = *rollup_config_size;
  if (MolReader_RollupConfig_verify(&config_seg, false) != MOL_OK) {
    ckb_debug("rollup config cell data is not RollupConfig format");
    return GW_FATAL_INVALID_DATA;
  }

  return 0;
}

/* Load challenge cell lock args */
int _load_challenge_lock_args(
    uint8_t rollup_script_hash[32], uint8_t challenge_script_type_hash[32],
    uint8_t challenge_script_buf[GW_MAX_CHALLENGE_LOCK_SCRIPT_SIZE],
    uint64_t source, uint64_t *index, mol_seg_t *lock_args) {
  uint64_t len;
  *index = 0;
  while (1) {
    /* load challenge lock script */
    len = GW_MAX_CHALLENGE_LOCK_SCRIPT_SIZE;
    int ret = ckb_checked_load_cell_by_field(
        challenge_script_buf, &len, 0, *index, source, CKB_CELL_FIELD_LOCK);
    if (ret != CKB_SUCCESS) {
      return ret;
    }
    mol_seg_t script_seg;
    script_seg.ptr = challenge_script_buf;
    script_seg.size = len;
    if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
      return GW_FATAL_INVALID_DATA;
    }

    /* check code_hash & hash type */
    mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
    mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);
    if (memcmp(code_hash_seg.ptr, challenge_script_type_hash, 32) == 0 &&
        *(uint8_t *)hash_type_seg.ptr == SCRIPT_HASH_TYPE_TYPE) {
      mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
      mol_seg_t raw_args_seg = MolReader_Bytes_raw_bytes(&args_seg);

      /* challenge lock script must start with a 32 bytes rollup script hash */
      if (raw_args_seg.size < 32) {
        ckb_debug("challenge lock script's args is less than 32 bytes");
        return GW_FATAL_INVALID_DATA;
      }
      if (memcmp(rollup_script_hash, raw_args_seg.ptr, 32) != 0) {
        ckb_debug("challenge lock script's rollup_script_hash mismatch");
        return GW_FATAL_INVALID_DATA;
      }

      /* the remain bytes of args is challenge lock args */
      lock_args->ptr = raw_args_seg.ptr + 32;
      lock_args->size = raw_args_seg.size - 32;
      if (MolReader_ChallengeLockArgs_verify(lock_args, false) != MOL_OK) {
        ckb_debug("invalid ChallengeLockArgs");
        return GW_FATAL_INVALID_DATA;
      }
      return 0;
    }
    *index += 1;
  }
}

/* Load verification context */
int _load_verification_context(
    uint8_t rollup_script_hash[32], uint64_t rollup_cell_index,
    uint64_t rollup_cell_source, uint64_t *challenge_cell_index,
    uint8_t challenged_block_hash[32], uint8_t block_merkle_root[32],
    uint32_t *tx_index, uint8_t rollup_config[GW_MAX_ROLLUP_CONFIG_SIZE],
    uint64_t *rollup_config_size) {

  /* load global state from rollup cell */
  uint8_t global_state_buf[sizeof(MolDefault_GlobalState)] = {0};
  uint64_t buf_len = sizeof(MolDefault_GlobalState);
  int ret = ckb_checked_load_cell_data(global_state_buf, &buf_len, 0,
                                       rollup_cell_index, rollup_cell_source);
  if (ret != 0) {
    return ret;
  }
  mol_seg_t global_state_seg;
  global_state_seg.ptr = global_state_buf;
  global_state_seg.size = buf_len;
  if (MolReader_GlobalState_verify(&global_state_seg, false) != MOL_OK) {
    ckb_debug("rollup cell data is not GlobalState format");
    return GW_FATAL_INVALID_DATA;
  }

  /* Get block_merkle_root */
  mol_seg_t block_merkle_state_seg =
      MolReader_GlobalState_get_block(&global_state_seg);
  mol_seg_t block_merkle_root_seg =
      MolReader_BlockMerkleState_get_merkle_root(&block_merkle_state_seg);
  if (block_merkle_root_seg.size != 32) {
    ckb_debug("invalid block merkle root");
    return GW_FATAL_INVALID_DATA;
  }
  memcpy(block_merkle_root, block_merkle_root_seg.ptr,
         block_merkle_root_seg.size);

  /* load rollup config cell */
  mol_seg_t rollup_config_hash_seg =
      MolReader_GlobalState_get_rollup_config_hash(&global_state_seg);
  ret = _load_rollup_config(rollup_config_hash_seg.ptr, rollup_config,
                            rollup_config_size);
  if (ret != 0) {
    ckb_debug("failed to load rollup_config_hash");
    return ret;
  }
  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = rollup_config;
  rollup_config_seg.size = *rollup_config_size;

  /* load challenge cell */
  mol_seg_t challenge_script_type_hash_seg =
      MolReader_RollupConfig_get_challenge_script_type_hash(&rollup_config_seg);

  uint8_t challenge_script_buf[GW_MAX_SCRIPT_SIZE] = {0};
  *challenge_cell_index = 0;
  mol_seg_t lock_args_seg;
  ret = _load_challenge_lock_args(rollup_script_hash,
                                  challenge_script_type_hash_seg.ptr,
                                  challenge_script_buf, CKB_SOURCE_INPUT,
                                  challenge_cell_index, &lock_args_seg);
  if (ret != 0) {
    ckb_debug("failed to load challenge lock args");
    return ret;
  }

  /* check challenge target_type */
  mol_seg_t target_seg = MolReader_ChallengeLockArgs_get_target(&lock_args_seg);

  /* get challenged block hash */
  mol_seg_t block_hash_seg =
      MolReader_ChallengeTarget_get_block_hash(&target_seg);
  if (block_hash_seg.size != 32) {
    ckb_debug("invalid challenged block hash");
    return GW_FATAL_INVALID_DATA;
  }
  memcpy(challenged_block_hash, block_hash_seg.ptr, block_hash_seg.size);

  /* check challenge type */
  mol_seg_t target_type_seg =
      MolReader_ChallengeTarget_get_target_type(&target_seg);
  uint8_t target_type = *(uint8_t *)target_type_seg.ptr;
  if (target_type != TARGET_TYPE_TRANSACTION) {
    ckb_debug("challenge target type is invalid");
    return GW_FATAL_INVALID_DATA;
  }
  /* get challenged transaction index */
  mol_seg_t tx_index_seg =
      MolReader_ChallengeTarget_get_target_index(&target_seg);
  *tx_index = *((uint32_t *)tx_index_seg.ptr);
  return 0;
}

/*
 * Load transaction checkpoints
 */
int _load_tx_checkpoint(mol_seg_t *raw_l2block_seg, uint32_t tx_index,
                        uint8_t prev_tx_checkpoint[32],
                        uint8_t post_tx_checkpoint[32]) {
  mol_seg_t submit_withdrawals_seg =
      MolReader_RawL2Block_get_submit_withdrawals(raw_l2block_seg);
  mol_seg_t withdrawals_count_seg =
      MolReader_SubmitWithdrawals_get_withdrawal_count(&submit_withdrawals_seg);
  uint32_t withdrawals_count = *((uint32_t *)withdrawals_count_seg.ptr);

  uint32_t prev_tx_checkpoint_index = withdrawals_count + tx_index - 1;
  uint32_t post_tx_checkpoint_index = withdrawals_count + tx_index;

  mol_seg_t checkpoint_list_seg =
      MolReader_RawL2Block_get_state_checkpoint_list(raw_l2block_seg);

  // load prev tx checkpoint
  if (0 == tx_index) {
    mol_seg_t submit_txs_seg =
        MolReader_RawL2Block_get_submit_transactions(raw_l2block_seg);
    mol_seg_t prev_state_checkpoint_seg =
        MolReader_SubmitTransactions_get_prev_state_checkpoint(&submit_txs_seg);
    if (32 != prev_state_checkpoint_seg.size) {
      ckb_debug("invalid prev state checkpoint");
      return GW_FATAL_INVALID_DATA;
    }
    memcpy(prev_tx_checkpoint, prev_state_checkpoint_seg.ptr, 32);
  } else {
    mol_seg_res_t checkpoint_res =
        MolReader_Byte32Vec_get(&checkpoint_list_seg, prev_tx_checkpoint_index);
    if (MOL_OK != checkpoint_res.errno || 32 != checkpoint_res.seg.size) {
      ckb_debug("invalid prev tx checkpoint");
      return GW_FATAL_INVALID_DATA;
    }
    memcpy(prev_tx_checkpoint, checkpoint_res.seg.ptr, 32);
  }

  // load post tx checkpoint
  mol_seg_res_t checkpoint_res =
      MolReader_Byte32Vec_get(&checkpoint_list_seg, post_tx_checkpoint_index);
  if (MOL_OK != checkpoint_res.errno || 32 != checkpoint_res.seg.size) {
    ckb_debug("invalid post tx checkpoint");
    return GW_FATAL_INVALID_DATA;
  }
  memcpy(post_tx_checkpoint, checkpoint_res.seg.ptr, 32);
  return 0;
}

/* Load verify transaction witness
 */
int _load_verify_transaction_witness(
    uint8_t rollup_script_hash[32], uint64_t challenge_cell_index,
    uint8_t challenged_block_hash[32], uint32_t tx_index,
    uint8_t block_merkle_root[32],
    gw_transaction_context_t *transaction_context, gw_block_info_t *block_info,
    gw_state_t *kv_state, gw_pair_t kv_pairs[GW_MAX_KV_PAIRS],
    uint8_t kv_state_proof[GW_MAX_KV_PROOF_SIZE], uint64_t *kv_state_proof_size,
    gw_script_entry_t scripts[GW_MAX_SCRIPT_ENTRIES_SIZE],
    uint64_t *script_entries_size, gw_account_merkle_state_t *prev_account,
    gw_account_merkle_state_t *post_account, uint8_t return_data_hash[32],
    gw_state_t *block_hashes_state,
    gw_pair_t block_hashes_pairs[GW_MAX_GET_BLOCK_HASH_DEPTH],
    uint8_t prev_tx_checkpoint[32], uint8_t post_tx_checkpoint[32]) {
  /* load witness from challenge cell */
  int ret;
  uint8_t buf[GW_MAX_WITNESS_SIZE];
  uint64_t buf_len = GW_MAX_WITNESS_SIZE;
  ret = ckb_checked_load_witness(buf, &buf_len, 0, challenge_cell_index,
                                 CKB_SOURCE_INPUT);
  if (ret != CKB_SUCCESS) {
    ckb_debug("load witness failed");
    return ret;
  }
  mol_seg_t witness_seg;
  witness_seg.ptr = buf;
  witness_seg.size = buf_len;
  if (MolReader_WitnessArgs_verify(&witness_seg, false) != MOL_OK) {
    ckb_debug("witness is not WitnessArgs format");
    return GW_FATAL_INVALID_DATA;
  }

  /* read VerifyTransactionWitness from witness_args.lock */
  mol_seg_t content_seg = MolReader_WitnessArgs_get_lock(&witness_seg);
  if (MolReader_BytesOpt_is_none(&content_seg)) {
    ckb_debug("WitnessArgs has no input field");
    return GW_FATAL_INVALID_DATA;
  }
  mol_seg_t verify_tx_witness_seg = MolReader_Bytes_raw_bytes(&content_seg);
  if (MolReader_VerifyTransactionWitness_verify(&verify_tx_witness_seg,
                                                false) != MOL_OK) {
    ckb_debug("input field is not VerifyTransactionWitness");
    return GW_FATAL_INVALID_DATA;
  }

  mol_seg_t raw_l2block_seg =
      MolReader_VerifyTransactionWitness_get_raw_l2block(
          &verify_tx_witness_seg);

  /* verify challenged block */
  uint8_t block_hash[32] = {0};

  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, raw_l2block_seg.ptr, raw_l2block_seg.size);
  blake2b_final(&blake2b_ctx, block_hash, 32);
  if (memcmp(block_hash, challenged_block_hash, 32) != 0) {
    ckb_debug("block hash mismatched with challenged block hash");
    return GW_FATAL_INVALID_DATA;
  }

  /* verify tx is challenge target */
  mol_seg_t l2tx_seg =
      MolReader_VerifyTransactionWitness_get_l2tx(&verify_tx_witness_seg);
  mol_seg_t raw_l2tx_seg = MolReader_L2Transaction_get_raw(&l2tx_seg);

  /* verify tx merkle proof */
  uint8_t tx_witness_hash[32] = {0};
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, l2tx_seg.ptr, l2tx_seg.size);
  blake2b_final(&blake2b_ctx, tx_witness_hash, 32);

  /* create a state to insert tx_witness_hash pair */
  gw_state_t txs_state;
  gw_pair_t txs_state_buffer[1] = {0};
  gw_state_init(&txs_state, txs_state_buffer, 1);
  uint8_t tx_key[32] = {0};
  memcpy(tx_key, (uint8_t *)&tx_index, 4);
  /* insert tx_index -> tx_witness_hash */
  ret = gw_state_insert(&txs_state, tx_key, tx_witness_hash);
  if (ret != 0) {
    return ret;
  }

  mol_seg_t submit_txs_seg =
      MolReader_RawL2Block_get_submit_transactions(&raw_l2block_seg);
  mol_seg_t tx_witness_root_seg =
      MolReader_SubmitTransactions_get_tx_witness_root(&submit_txs_seg);
  mol_seg_t tx_proof_seg =
      MolReader_VerifyTransactionWitness_get_tx_proof(&verify_tx_witness_seg);
  mol_seg_t raw_tx_proof_seg = MolReader_Bytes_raw_bytes(&tx_proof_seg);
  gw_state_normalize(&txs_state);
  ret = gw_smt_verify(tx_witness_root_seg.ptr, &txs_state, raw_tx_proof_seg.ptr,
                      raw_tx_proof_seg.size);
  if (ret != 0) {
    ckb_debug("failed to merkle verify tx witness root");
    return ret;
  }

  /* load transaction context */
  ret = gw_parse_transaction_context(transaction_context, &raw_l2tx_seg);
  if (ret != 0) {
    ckb_debug("parse l2 transaction failed");
    return ret;
  }

  /* load block info */
  mol_seg_t number_seg = MolReader_RawL2Block_get_number(&raw_l2block_seg);
  uint64_t challenged_block_number = *(uint64_t *)number_seg.ptr;
  mol_seg_t timestamp_seg =
      MolReader_RawL2Block_get_timestamp(&raw_l2block_seg);
  mol_seg_t block_producer_id_seg =
      MolReader_RawL2Block_get_block_producer_id(&raw_l2block_seg);
  block_info->number = *((uint64_t *)number_seg.ptr);
  block_info->timestamp = *((uint64_t *)timestamp_seg.ptr);
  block_info->block_producer_id = *((uint32_t *)block_producer_id_seg.ptr);

  /* Load VerifyTransactionContext */
  mol_seg_t verify_tx_ctx_seg =
      MolReader_VerifyTransactionWitness_get_context(&verify_tx_witness_seg);

  /* load block hashes */
  mol_seg_t block_hashes_seg =
      MolReader_VerifyTransactionContext_get_block_hashes(&verify_tx_ctx_seg);
  uint32_t block_hashes_size =
      MolReader_BlockHashEntryVec_length(&block_hashes_seg);
  gw_state_init(block_hashes_state, block_hashes_pairs,
                GW_MAX_GET_BLOCK_HASH_DEPTH);
  uint64_t max_block_number = 0;
  if (challenged_block_number > 1) {
    max_block_number = challenged_block_number - 1;
  }
  uint64_t min_block_number = 0;
  if (challenged_block_number > GW_MAX_GET_BLOCK_HASH_DEPTH) {
    min_block_number = challenged_block_number - GW_MAX_GET_BLOCK_HASH_DEPTH;
  }

  for (uint32_t i = 0; i < block_hashes_size; i++) {
    mol_seg_res_t block_hash_entry_res =
        MolReader_BlockHashEntryVec_get(&block_hashes_seg, i);
    if (block_hash_entry_res.errno != MOL_OK) {
      ckb_debug("invalid block hash entry");
      return GW_FATAL_INVALID_DATA;
    }
    mol_seg_t num_seg =
        MolReader_BlockHashEntry_get_number(&block_hash_entry_res.seg);
    uint64_t block_number = *(uint64_t *)num_seg.ptr;
    if (block_number < min_block_number || block_number > max_block_number) {
      ckb_debug("invalid number in block hashes");
      return GW_FATAL_INVALID_DATA;
    }
    mol_seg_t hash_seg =
        MolReader_BlockHashEntry_get_hash(&block_hash_entry_res.seg);
    uint8_t key[32] = {0};
    _gw_block_smt_key(key, block_number);
    ret = gw_state_insert(block_hashes_state, key, hash_seg.ptr);
    if (ret != 0) {
      return ret;
    }
  }
  /* Merkle proof */
  if (block_hashes_size > 0) {
    mol_seg_t block_hashes_proof_seg =
        MolReader_VerifyTransactionWitness_get_block_hashes_proof(
            &verify_tx_witness_seg);
    gw_state_normalize(block_hashes_state);
    ret =
        gw_smt_verify(block_merkle_root, block_hashes_state,
                      block_hashes_proof_seg.ptr, block_hashes_proof_seg.size);
    if (ret != 0) {
      ckb_debug("failed to verify block merkle root and block hashes");
      return ret;
    }
  }

  /* load kv state */
  mol_seg_t kv_state_seg =
      MolReader_VerifyTransactionContext_get_kv_state(&verify_tx_ctx_seg);
  uint32_t kv_pairs_len = MolReader_KVPairVec_length(&kv_state_seg);
  if (kv_pairs_len > GW_MAX_KV_PAIRS) {
    ckb_debug("too many key/value pair");
    return GW_FATAL_INVALID_DATA;
  }
  /* initialize kv state */
  gw_state_init(kv_state, kv_pairs, GW_MAX_KV_PAIRS);
  for (uint32_t i = 0; i < kv_pairs_len; i++) {
    mol_seg_res_t kv_res = MolReader_KVPairVec_get(&kv_state_seg, i);
    if (kv_res.errno != MOL_OK) {
      ckb_debug("invalid kv pairs");
      return GW_FATAL_INVALID_DATA;
    }
    mol_seg_t key_seg = MolReader_KVPair_get_k(&kv_res.seg);
    mol_seg_t value_seg = MolReader_KVPair_get_v(&kv_res.seg);
    ret = gw_state_insert(kv_state, key_seg.ptr, value_seg.ptr);
    if (ret != 0) {
      return ret;
    }
  }

  /* load kv state proof */
  mol_seg_t kv_state_proof_seg =
      MolReader_VerifyTransactionWitness_get_kv_state_proof(
          &verify_tx_witness_seg);
  mol_seg_t kv_state_proof_bytes_seg =
      MolReader_Bytes_raw_bytes(&kv_state_proof_seg);
  if (kv_state_proof_bytes_seg.size > GW_MAX_KV_PROOF_SIZE) {
    ckb_debug("kv state proof is too long");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  memcpy(kv_state_proof, kv_state_proof_bytes_seg.ptr,
         kv_state_proof_bytes_seg.size);
  *kv_state_proof_size = kv_state_proof_bytes_seg.size;

  /* load tx checkpoint */
  ret = _load_tx_checkpoint(&raw_l2block_seg, tx_index, prev_tx_checkpoint,
                            post_tx_checkpoint);
  if (ret != 0) {
    return ret;
  }

  /* load prev account state */
  mol_seg_t prev_account_seg =
      MolReader_RawL2Block_get_prev_account(&raw_l2block_seg);
  mol_seg_t prev_merkle_root_seg =
      MolReader_AccountMerkleState_get_merkle_root(&prev_account_seg);
  mol_seg_t prev_count_seg =
      MolReader_AccountMerkleState_get_count(&prev_account_seg);
  memcpy(prev_account->merkle_root, prev_merkle_root_seg.ptr, 32);
  prev_account->count = *((uint32_t *)prev_count_seg.ptr);
  /* load post account state */
  mol_seg_t post_account_seg =
      MolReader_RawL2Block_get_post_account(&raw_l2block_seg);
  mol_seg_t post_merkle_root_seg =
      MolReader_AccountMerkleState_get_merkle_root(&post_account_seg);
  mol_seg_t post_count_seg =
      MolReader_AccountMerkleState_get_count(&post_account_seg);
  memcpy(post_account->merkle_root, post_merkle_root_seg.ptr, 32);
  post_account->count = *((uint32_t *)post_count_seg.ptr);

  /* load scripts */
  mol_seg_t scripts_seg =
      MolReader_VerifyTransactionContext_get_scripts(&verify_tx_ctx_seg);
  uint32_t entries_size = MolReader_ScriptVec_length(&scripts_seg);
  if (entries_size > GW_MAX_SCRIPT_ENTRIES_SIZE) {
    ckb_debug("script size is exceeded maximum");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  *script_entries_size = 0;
  for (uint32_t i = 0; i < entries_size; i++) {
    gw_script_entry_t entry = {0};
    mol_seg_res_t script_res = MolReader_ScriptVec_get(&scripts_seg, i);
    if (script_res.errno != MOL_OK) {
      ckb_debug("invalid script entry format");
      return GW_FATAL_INVALID_DATA;
    }
    if (script_res.seg.size > GW_MAX_SCRIPT_SIZE) {
      ckb_debug("invalid script entry format");
      return GW_FATAL_INVALID_DATA;
    }

    /* copy script to entry */
    memcpy(entry.script, script_res.seg.ptr, script_res.seg.size);
    entry.script_len = script_res.seg.size;

    /* copy script hash to entry */
    blake2b_state blake2b_ctx;
    blake2b_init(&blake2b_ctx, 32);
    blake2b_update(&blake2b_ctx, script_res.seg.ptr, script_res.seg.size);
    blake2b_final(&blake2b_ctx, entry.hash, 32);

    /* insert entry */
    memcpy(&scripts[*script_entries_size], &entry, sizeof(gw_script_entry_t));
    *script_entries_size += 1;
  }

  /* load return data hash */
  mol_seg_t return_data_hash_seg =
      MolReader_VerifyTransactionContext_get_return_data_hash(
          &verify_tx_ctx_seg);
  memcpy(return_data_hash, return_data_hash_seg.ptr, 32);

  return 0;
}

/* check that an account script is allowed */
int _gw_check_account_script_is_allowed(uint8_t rollup_script_hash[32],
                                        mol_seg_t *script_seg,
                                        mol_seg_t *rollup_config_seg) {

  if (MolReader_Script_verify(script_seg, false) != MOL_OK) {
    ckb_debug("disallow script because of the format is invalid");
    return GW_FATAL_INVALID_DATA;
  }

  if (script_seg->size > GW_MAX_SCRIPT_SIZE) {
    ckb_debug("disallow script because of size is too large");
    return GW_FATAL_INVALID_DATA;
  }

  /* check hash type */
  mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(script_seg);
  if (*(uint8_t *)hash_type_seg.ptr != SCRIPT_HASH_TYPE_TYPE) {
    ckb_debug("disallow script because of script hash type is not 'type'");
    return GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH;
  }
  mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(script_seg);
  if (code_hash_seg.size != 32) {
    return GW_FATAL_INVALID_DATA;
  }

  /* check allowed EOA list */
  mol_seg_t eoa_list_seg =
      MolReader_RollupConfig_get_allowed_eoa_type_hashes(rollup_config_seg);
  uint32_t len = MolReader_Byte32Vec_length(&eoa_list_seg);
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t allowed_code_hash_res =
        MolReader_Byte32Vec_get(&eoa_list_seg, i);
    if (allowed_code_hash_res.errno != MOL_OK ||
        allowed_code_hash_res.seg.size != code_hash_seg.size) {
      ckb_debug("disallow script because eoa code_hash is invalid");
      return GW_FATAL_INVALID_DATA;
    }
    if (memcmp(allowed_code_hash_res.seg.ptr, code_hash_seg.ptr,
               code_hash_seg.size) == 0) {
      /* found a valid code_hash */
      return 0;
    }
  }

  /* check allowed contract list */
  mol_seg_t contract_list_seg =
      MolReader_RollupConfig_get_allowed_contract_type_hashes(
          rollup_config_seg);
  len = MolReader_Byte32Vec_length(&contract_list_seg);
  for (uint32_t i = 0; i < len; i++) {
    mol_seg_res_t allowed_code_hash_res =
        MolReader_Byte32Vec_get(&contract_list_seg, i);
    if (allowed_code_hash_res.errno != MOL_OK ||
        allowed_code_hash_res.seg.size != code_hash_seg.size) {
      ckb_debug("disallow script because contract code_hash is invalid");
      return GW_FATAL_INVALID_DATA;
    }
    if (memcmp(allowed_code_hash_res.seg.ptr, code_hash_seg.ptr,
               code_hash_seg.size) == 0) {
      // check that contract'script must start with a 32 bytes
      // rollup_script_hash
      mol_seg_t args_seg = MolReader_Script_get_args(script_seg);
      mol_seg_t raw_args_seg = MolReader_Bytes_raw_bytes(&args_seg);
      if (raw_args_seg.size < 32) {
        ckb_debug(
            "disallow contract script because args is less than 32 bytes");
        return GW_ERROR_INVALID_CONTRACT_SCRIPT;
      }
      if (memcmp(rollup_script_hash, raw_args_seg.ptr, 32) != 0) {
        ckb_debug("disallow contract script because args is not start with "
                  "rollup_script_hash");
        return GW_ERROR_INVALID_CONTRACT_SCRIPT;
      }
      /* found a valid code_hash */
      return 0;
    }
  }

  /* script is not allowed */
  ckb_debug("disallow script because code_hash is unknown");
  return GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH;
}

/* block smt key */
void _gw_block_smt_key(uint8_t key[32], uint64_t number) {
  memcpy(key, (uint8_t *)&number, 8);
}

/*
 * To prevent others consume the cell,
 * an owner_lock_hash(32 bytes) is put in the current cell's data,
 * this function checks that at least an input cell's lock_hash equals to the
 * owner_lock_hash, thus, we can make sure current cell is unlocked by the
 * owner, otherwise this function return an error.
 */
int _check_owner_lock_hash() {
  /* read data from current cell */
  uint8_t owner_lock_hash[32] = {0};
  uint64_t len = 32;
  int ret =
      ckb_load_cell_data(owner_lock_hash, &len, 0, 0, CKB_SOURCE_GROUP_INPUT);
  if (ret != 0) {
    printf("check owner lock hash failed, can't load cell data, ret: %d", ret);
    return ret;
  }
  if (len != 32) {
    printf("check owner lock hash failed, invalid data len: %ld", len);
    return GW_FATAL_INVALID_DATA;
  }
  /* look for owner cell */
  size_t current = 0;
  while (true) {
    len = 32;
    uint8_t lock_hash[32] = {0};

    ret = ckb_load_cell_by_field(lock_hash, &len, 0, current, CKB_SOURCE_INPUT,
                                 CKB_CELL_FIELD_LOCK_HASH);

    if (ret != 0) {
      return ret;
    }
    if (memcmp(lock_hash, owner_lock_hash, 32) == 0) {
      /* found owner lock cell */
      return 0;
    }
    current++;
  }
  return CKB_INDEX_OUT_OF_BOUND;
}

int gw_context_init(gw_context_t *ctx) {
  /* check owner lock */
  int ret = _check_owner_lock_hash();
  if (ret != 0) {
    return ret;
  }

  /* setup syscalls */
  ctx->sys_load = sys_load;
  ctx->sys_store = sys_store;
  ctx->sys_set_program_return_data = sys_set_program_return_data;
  ctx->sys_create = sys_create;
  ctx->sys_get_account_id_by_script_hash = sys_get_account_id_by_script_hash;
  ctx->sys_get_script_hash_by_account_id = sys_get_script_hash_by_account_id;
  ctx->sys_get_account_nonce = sys_get_account_nonce;
  ctx->sys_get_account_script = sys_get_account_script;
  ctx->sys_store_data = sys_store_data;
  ctx->sys_load_data = sys_load_data;
  ctx->sys_get_block_hash = sys_get_block_hash;
  ctx->sys_get_script_hash_by_prefix = sys_get_script_hash_by_prefix;
  ctx->sys_recover_account = sys_recover_account;
  ctx->sys_log = sys_log;
  ctx->sys_pay_fee = sys_pay_fee;
  ctx->_internal_load_raw = _internal_load_raw;
  ctx->_internal_store_raw = _internal_store_raw;

  /* initialize context */
  uint8_t rollup_script_hash[32] = {0};
  ret = _load_rollup_script_hash(rollup_script_hash);
  if (ret != 0) {
    ckb_debug("failed to load rollup script hash");
    return ret;
  }
  /* set ctx->rollup_script_hash */
  memcpy(ctx->rollup_script_hash, rollup_script_hash, 32);
  uint64_t rollup_cell_index = 0;
  ret = _find_cell_by_type_hash(rollup_script_hash, CKB_SOURCE_INPUT,
                                &rollup_cell_index);
  if (ret == CKB_INDEX_OUT_OF_BOUND) {
    /* exit execution with 0 if we are not in a challenge */
    ckb_debug("can't found rollup cell from inputs which means we are not in a "
              "challenge, unlock cell without execution script");
    ckb_exit(0);
  } else if (ret != 0) {
    ckb_debug("failed to load rollup cell index");
    return ret;
  }
  uint64_t challenge_cell_index = 0;
  uint8_t challenged_block_hash[32] = {0};
  uint8_t block_merkle_root[32] = {0};
  ret = _load_verification_context(
      rollup_script_hash, rollup_cell_index, CKB_SOURCE_INPUT,
      &challenge_cell_index, challenged_block_hash, block_merkle_root,
      &ctx->tx_index, ctx->rollup_config, &ctx->rollup_config_size);
  if (ret != 0) {
    ckb_debug("failed to load verification context");
    return ret;
  }

  /* load context fields */
  ret = _load_verify_transaction_witness(
      rollup_script_hash, challenge_cell_index, challenged_block_hash,
      ctx->tx_index, block_merkle_root, &ctx->transaction_context,
      &ctx->block_info, &ctx->kv_state, ctx->kv_pairs, ctx->kv_state_proof,
      &ctx->kv_state_proof_size, ctx->scripts, &ctx->script_entries_size,
      &ctx->prev_account, &ctx->post_account, ctx->return_data_hash,
      &ctx->block_hashes_state, ctx->block_hashes_pairs,
      ctx->prev_tx_checkpoint, ctx->post_tx_checkpoint);
  if (ret != 0) {
    ckb_debug("failed to load verify transaction witness");
    return ret;
  }
  /* set current account count */
  ctx->account_count = ctx->prev_account.count;

  /* verify kv_state merkle proof */
  gw_state_normalize(&ctx->kv_state);
  ret = gw_smt_verify(ctx->prev_account.merkle_root, &ctx->kv_state,
                      ctx->kv_state_proof, ctx->kv_state_proof_size);
  if (ret != 0) {
    ckb_debug("failed to merkle verify prev account merkle root");
    return ret;
  }

  /* init original sender nonce */
  ret = _load_sender_nonce(ctx, &ctx->original_sender_nonce);
  if(ret != 0) {
    ckb_debug("failed to init original sender nonce");
    return ret;
  }

  return 0;
}

int gw_finalize(gw_context_t *ctx) {
  if (ctx->post_account.count != ctx->account_count) {
    ckb_debug("account count not match");
    return GW_FATAL_INVALID_DATA;
  }

  /* update sender nonce */
  int ret = _increase_sender_nonce(ctx);
  if(ret != 0) {
    ckb_debug("failed to update original sender nonce");
    return ret;
  }

  uint8_t return_data_hash[32] = {0};
  blake2b_state blake2b_ctx;
  blake2b_init(&blake2b_ctx, 32);
  blake2b_update(&blake2b_ctx, ctx->receipt.return_data,
                 ctx->receipt.return_data_len);
  blake2b_final(&blake2b_ctx, return_data_hash, 32);
  if (memcmp(return_data_hash, ctx->return_data_hash, 32) != 0) {
    ckb_debug("return data hash not match");
    return GW_FATAL_MISMATCH_RETURN_DATA;
  }

  gw_state_normalize(&ctx->kv_state);
  ret = gw_smt_verify(ctx->post_account.merkle_root, &ctx->kv_state,
                          ctx->kv_state_proof, ctx->kv_state_proof_size);
  if (ret != 0) {
    ckb_debug("failed to merkle verify post account merkle root");
    return ret;
  }
  return 0;
}

int gw_verify_sudt_account(gw_context_t *ctx, uint32_t sudt_id) {
  uint8_t script_buffer[GW_MAX_SCRIPT_SIZE];
  uint64_t script_len = GW_MAX_SCRIPT_SIZE;
  int ret = sys_get_account_script(ctx, sudt_id, &script_len, 0, script_buffer);
  if (ret != 0) {
    return ret;
  }
  mol_seg_t script_seg;
  script_seg.ptr = script_buffer;
  script_seg.size = script_len;
  if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
    ckb_debug("load account script: invalid script");
    return GW_FATAL_INVALID_SUDT_SCRIPT;
  }
  mol_seg_t code_hash_seg = MolReader_Script_get_code_hash(&script_seg);
  mol_seg_t hash_type_seg = MolReader_Script_get_hash_type(&script_seg);

  mol_seg_t rollup_config_seg;
  rollup_config_seg.ptr = ctx->rollup_config;
  rollup_config_seg.size = ctx->rollup_config_size;
  mol_seg_t l2_sudt_validator_script_type_hash =
      MolReader_RollupConfig_get_l2_sudt_validator_script_type_hash(
          &rollup_config_seg);
  if (memcmp(l2_sudt_validator_script_type_hash.ptr, code_hash_seg.ptr, 32) !=
      0) {
    return GW_FATAL_INVALID_SUDT_SCRIPT;
  }
  if (*hash_type_seg.ptr != 1) {
    return GW_FATAL_INVALID_SUDT_SCRIPT;
  }
  return 0;
}
#endif
