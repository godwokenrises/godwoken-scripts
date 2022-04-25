#ifndef GW_UINT256_H_
#define GW_UINT256_H_

#define MAX_UINT32 ((uint64_t)0xFFFFFFFF)

typedef struct uint256_t {
  uint32_t array[8];
} uint256_t;

int uint256_from_little_endian(uint8_t* bytes, int size, uint256_t* num) {
  if (size < 32) {
    return 1;
  }

  for (int i = 0; i < 8; ++i) {
    num->array[i] = *(uint32_t*)&bytes[i * 4];
  }

  return 0;
}

int uint256_to_little_endian(const uint256_t num, uint8_t* bytes, int size) {
  if (size < 32) {
    return 1;
  }

  for (int i = 0; i < 8; ++i) {
    *(uint32_t*)&bytes[i * 4] = num.array[i];
  }

  return 0;
}

void uint256_zero(uint256_t* num) {
  for (int i = 0; i < 8; ++i) {
    num->array[i] = 0;
  }
}

void uint256_one(uint256_t* num) {
  uint256_zero(num);
  num->array[0] = 1;
}

void uint256_max(uint256_t* num) {
  for (int i = 0; i < 8; ++i) {
    num->array[i] = (uint32_t)0xFFFFFFFF;
  }
}

int uint256_overflow_add(const uint256_t a, const uint256_t b, uint256_t* sum) {
  uint64_t tmp;

  int carry = 0;
  uint256_zero(sum);

  for (int i = 0; i < 8; ++i) {
    tmp = (uint64_t)a.array[i] + b.array[i] + carry;
    carry = (tmp > MAX_UINT32);
    sum->array[i] = (tmp & MAX_UINT32);
  }

  return carry;
}

int uint256_underflow_sub(const uint256_t a, const uint256_t b,
                          uint256_t* rem) {
  uint64_t res;
  uint64_t tmp1;
  uint64_t tmp2;

  int borrow = 0;
  uint256_zero(rem);

  for (int i = 0; i < 8; ++i) {
    tmp1 = (uint64_t)a.array[i] + (MAX_UINT32 + 1);
    tmp2 = (uint64_t)b.array[i] + borrow;
    res = (tmp1 - tmp2);
    rem->array[i] = (uint32_t)(res & MAX_UINT32);
    borrow = (res <= MAX_UINT32);
  }

  return borrow;
}

enum { SMALLER = -1, EQUAL = 0, LARGER = 1 };

int uint256_cmp(const uint256_t a, const uint256_t b) {
  for (int i = 7; i >= 0; --i) {
    if (a.array[i] > b.array[i]) {
      return LARGER;
    } else if (a.array[i] < b.array[i]) {
      return SMALLER;
    }
  }

  return EQUAL;
}

#endif
