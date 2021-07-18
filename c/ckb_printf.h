#ifndef __CKB_PRINTF__
#define __CKB_PRINTF__
// printf, pass -D CKB_C_STDLIB_PRINTF to enable printf
// default: disabled
#ifdef CKB_C_STDLIB_PRINTF

int vsnprintf_(char *buffer, size_t count, const char *format, va_list va);

// syscall
int ckb_debug(const char *s);
int ckb_printf(const char *format, ...) {
  static char buf[CKB_C_STDLIB_PRINTF_BUFFER_SIZE];
  va_list va;
  va_start(va, format);
  int ret = vsnprintf_(buf, CKB_C_STDLIB_PRINTF_BUFFER_SIZE, format, va);
  va_end(va);
  ckb_debug(buf);
  return ret;
}

#else

int ckb_printf(const char *format, ...) { return 0; }

#endif /* CKB_C_STDLIB_PRINTF */

#endif  // __CKB_PRINTF__
