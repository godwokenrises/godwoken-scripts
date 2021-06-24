#ifndef GW_DEF_H_
#define GW_DEF_H_

#include "stddef.h"

typedef unsigned __int128 uint128_t;

#define GW_KEY_BYTES 32
#define GW_VALUE_BYTES 32

/* Key type */
#define GW_ACCOUNT_KV 0
#define GW_ACCOUNT_NONCE 1
#define GW_ACCOUNT_SCRIPT_HASH 2
/* Non account type */
#define GW_ACCOUNT_SCRIPT_HASH_TO_ID 3
#define GW_DATA_HASH_PREFIX 4

/* Limitations */
/* 24KB (ethereum max contract code size) */
#define GW_MAX_RETURN_DATA_SIZE (24 * 1024)
/* 128KB */
#define GW_MAX_L2TX_ARGS_SIZE (128 * 1024)
/* 128KB + 4KB */
#define GW_MAX_L2TX_SIZE (132 * 1024)
/* MAX kv state pairs in a tx */
#define GW_MAX_KV_PAIRS 1024
#define GW_MAX_SCRIPT_SIZE 256
/* MAX scripts in a tx */
#define GW_MAX_SCRIPT_ENTRIES_SIZE 100
/* MAX size of rollup config */
#define GW_MAX_ROLLUP_CONFIG_SIZE (4 * 1024)
#define GW_MAX_WITNESS_SIZE (300 * 1024)
#define GW_MAX_CODE_SIZE (64 * 1024)

#define GW_LOG_SUDT_TRANSFER    0x0
#define GW_LOG_SUDT_PAY_FEE     0x1
#define GW_LOG_POLYJUICE_SYSTEM 0x2
#define GW_LOG_POLYJUICE_USER   0x3

/* Godwoken context */
typedef struct {
  uint32_t from_id;
  uint32_t to_id;
  uint8_t args[GW_MAX_L2TX_ARGS_SIZE];
  uint32_t args_len;
} gw_transaction_context_t;

typedef struct {
  uint64_t number;
  uint64_t timestamp;
  uint32_t block_producer_id;
} gw_block_info_t;

struct gw_context_t;

/**
 * Initialize Godwoken context
 */
int gw_context_init(struct gw_context_t *ctx);

/**
 * Finalize Godwoken state
 */
int gw_finalize(struct gw_context_t *ctx);

/**
 * Verify sudt account
 */
int gw_verify_sudt_account(struct gw_context_t *ctx, uint32_t sudt_id);


/* layer2 syscalls */

/**
 * Create a new account
 *
 * @param ctx        The godwoken context
 * @param script     Contract's script (MUST be valid molecule format CKB
 * Script)
 * @param script_len Length of script structure
 * @param account_id ID of new account
 * @return           The status code, 0 is success
 */
typedef int (*gw_create_fn)(struct gw_context_t *ctx, uint8_t *script,
                            uint64_t script_len, uint32_t *account_id);

/**
 * Load value by key from current contract account
 *
 * @param ctx        The godwoken context
 * @param account_id account to modify
 * @param key        The key (less than 32 bytes)
 * @param key_len    The key length (less then 32)
 * @param value      The pointer to save the value of the key (32 bytes)
 * @return           The status code, 0 is success
 */
typedef int (*gw_load_fn)(struct gw_context_t *ctx, uint32_t account_id,
                          const uint8_t *key,
                          const uint64_t key_len,
                          uint8_t value[GW_VALUE_BYTES]);

/**
 * Store key,value pair to current account's storage
 *
 * @param ctx        The godwoken context
 * @param account_id account to read
 * @param key        The key (less than 32 bytes)
 * @param key_len    The key length (less then 32)
 * @param value      The value
 * @return           The status code, 0 is success
 */
typedef int (*gw_store_fn)(struct gw_context_t *ctx, uint32_t account_id,
                           const uint8_t *key,
                           const uint64_t key_len,
                           const uint8_t value[GW_VALUE_BYTES]);

/**
 * Set the return data of current layer 2 contract (program) execution
 *
 * @param data   The data to return
 * @param len    The length of return data
 * @return       The status code, 0 is success
 */
typedef int (*gw_set_program_return_data_fn)(struct gw_context_t *ctx,
                                             uint8_t *data, uint64_t len);

/**
 * Get account id by account script_hash
 *
 * @param ctx        The godwoken context
 * @param script_hashThe account script_hash
 * @param account_id The pointer of the account id to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_account_id_by_script_hash_fn)(struct gw_context_t *ctx,
                                                   uint8_t script_hash[32],
                                                   uint32_t *account_id);

/**
 * Get account script_hash by account id
 *
 * @param ctx        The godwoken context
 * @param account_id The account id
 * @param script_hashThe pointer of the account script hash to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_script_hash_by_account_id_fn)(struct gw_context_t *ctx,
                                                   uint32_t account_id,
                                                   uint8_t script_hash[32]);

/**
 * Get account's nonce
 *
 * @param ctx        The godwoken context
 * @param account_id The account id
 * @param nonce      The point of the nonce to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_account_nonce_fn)(struct gw_context_t *ctx,
                                       uint32_t account_id, uint32_t *nonce);

/**
 * Get account script by account id
 */
typedef int (*gw_get_account_script_fn)(struct gw_context_t *ctx,
                                        uint32_t account_id, uint64_t *len,
                                        uint64_t offset, uint8_t *script);
/**
 * Load data by data hash
 *
 * @param ctx        The godwoken context
 * @param data_hash  The data hash (hash = ckb_blake2b(data))
 * @param len        The length of the script data
 * @param offset     The offset of the script data
 * @param data       The pointer of the data to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_load_data_fn)(struct gw_context_t *ctx, uint8_t data_hash[32],
                               uint64_t *len, uint64_t offset, uint8_t *data);

typedef int (*gw_store_data_fn)(struct gw_context_t *ctx, uint64_t data_len,
                                uint8_t *data);

/**
 * Get layer 2 block hash by number
 *
 * @param ctx        The godwoken context
 * @param number     The number of the layer 2 block
 * @param block_hash The pointer of the layer 2 block hash to save the result
 * @return           The status code, 0 is success
 */
typedef int (*gw_get_block_hash_fn)(struct gw_context_t *ctx, uint64_t number,
                                    uint8_t block_hash[32]);

/**
 * Get account script hash by prefix (short address)
 *
 * @param ctx         The godwoken context
 * @param prefix      The pointer of prefix data
 * @param prefix_len  The length of prefix data
 * @param script_hash The account script hash
 * @return            The status code, 0 is success
 */
typedef int (*gw_get_script_hash_by_prefix_fn)(struct gw_context_t *ctx,
                                               uint8_t *prefix, uint64_t prefix_len,
                                               uint8_t script_hash[32]);
/**
 * Recover an EoA account script by signature
 *
 * @param ctx            The godwoken context
 * @param message        The message of corresponding signature
 * @param signature      The pointer of signature data
 * @param signature_len  The length of signature data
 * @param code_hash      The EoA account script's code_hash
 * @param script         The pointer of script data
 * @param script_len     The pointer to length of script data
 * @return               The status code, 0 is success
 */

typedef int (*gw_recover_account_fn)(struct gw_context_t *ctx,
                                     uint8_t message[32],
                                     uint8_t *signature,
                                     uint64_t signature_len,
                                     uint8_t code_hash[32],
                                     uint8_t *script,
                                     uint64_t *script_len);

/**
 * Emit a log (EVM LOG0, LOG1, LOGn in polyjuice)
 *
 * @param ctx            The godwoken context
 * @param account_id     The account to emit log
 * @param service_flag   The service flag of log, for category different log types
 * @param data           The log data
 * @param data_length    The length of the log data
 * @return               The status code, 0 is success
 */
typedef int (*gw_log_fn)(struct gw_context_t *ctx, uint32_t account_id, uint8_t service_flag,
                         uint64_t data_length, const uint8_t *data);

/**
 * Record fee payment
 *
 * @param payer_addr     Memory addr of payer short address
 * @param short_addr_len Length of payer short address
 * @param sudt_id        Account id of sUDT
 * @param amount         The amount of fee
 * @return               The status code, 0 is success
 */
typedef int (*gw_pay_fee_fn)(struct gw_context_t *ctx, const uint8_t *payer_addr,
                             const uint64_t short_addr_len, uint32_t sudt_id, uint128_t amount);

#endif /* GW_DEF_H_ */
