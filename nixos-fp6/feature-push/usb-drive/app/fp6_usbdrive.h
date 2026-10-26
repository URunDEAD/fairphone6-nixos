// C ABI of the Rust backend (rust/src/ffi.rs).
#pragma once
#ifdef __cplusplus
extern "C" {
#endif
typedef struct Fp6UsbDrive Fp6UsbDrive;
Fp6UsbDrive *fp6_usbdrive_new(void);
char *fp6_usbdrive_command(Fp6UsbDrive *h, const char *name, const char *arg_json);
void fp6_usbdrive_free(Fp6UsbDrive *h);
void fp6_usbdrive_string_free(char *s);
#ifdef __cplusplus
}
#endif
