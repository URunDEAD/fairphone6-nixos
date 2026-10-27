#include <stdio.h>
#include <stdarg.h>
#include <stdint.h>
typedef char char_t;
void ar_log_init(void) {}
void ar_log_deinit(void) {}
void ar_set_log_level(uint32_t level) {}
void ar_log(uint32_t level, const char_t *tag, const char_t *file, const char_t *fn, const uint32_t ln, const char_t *fmt, ...)
{
	if (level > 2) return;
	va_list a; va_start(a, fmt); fprintf(stderr, "[%u %s:%u] ", level, fn, ln); vfprintf(stderr, fmt, a); fputc('\n', stderr); va_end(a);
}
uint32_t ar_log_lvl = 0x3;
