/* SPDX-License-Identifier: MIT
 *
 * FP6 header-only replacement for libhybris' <hybris/properties/properties.h>.
 *
 * lomiri-system-settings only builds its About and Brightness panels when
 * libandroid-properties is found, but both use property_get() just for
 * cosmetic strings (ro.product.*, ro.serialno, ro.build.display.id,
 * ubuntu.widi.supported). On NixOS there is no Android property service, so
 * this shim answers from a plain "key=value" file (FP6_PROPERTIES_FILE,
 * installed by settings-fp6.nix) and otherwise returns the caller's default.
 * Read-only; never sets anything.
 */
#ifndef FP6_ANDROID_PROPERTIES_SHIM_H
#define FP6_ANDROID_PROPERTIES_SHIM_H

#include <stdio.h>
#include <string.h>

#define PROP_NAME_MAX 32
#define PROP_VALUE_MAX 92

#ifndef FP6_PROPERTIES_FILE
#define FP6_PROPERTIES_FILE "/etc/lomiri-system-settings/properties"
#endif

static inline int property_get(const char *key, char *value, const char *default_value)
{
    char line[PROP_NAME_MAX + PROP_VALUE_MAX + 64];
    size_t keylen = key ? strlen(key) : 0;
    FILE *f;

    value[0] = '\0';
    if (default_value) {
        snprintf(value, PROP_VALUE_MAX, "%.*s", PROP_VALUE_MAX - 1, default_value);
    }
    if (!keylen || !(f = fopen(FP6_PROPERTIES_FILE, "re")))
        return (int)strlen(value);

    while (fgets(line, sizeof line, f)) {
        if (strncmp(line, key, keylen) == 0 && line[keylen] == '=') {
            char *v = line + keylen + 1;
            v[strcspn(v, "\r\n")] = '\0';
            snprintf(value, PROP_VALUE_MAX, "%.*s", PROP_VALUE_MAX - 1, v);
            break;
        }
    }
    fclose(f);
    return (int)strlen(value);
}

#endif
