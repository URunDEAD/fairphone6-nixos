// C ABI of the Rust backend (rust/src/ffi.rs).
#pragma once
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct Fp6Feed Fp6Feed;
Fp6Feed *fp6_feed_new(const char *kind);
void fp6_feed_set_active(Fp6Feed *feed, bool on);
char *fp6_feed_take(Fp6Feed *feed);
char *fp6_feed_command(Fp6Feed *feed, const char *name, const char *arg);
void fp6_feed_free(Fp6Feed *feed);
void fp6_string_free(char *s);
#ifdef __cplusplus
}
#endif
