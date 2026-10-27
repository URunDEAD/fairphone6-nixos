// SPDX-License-Identifier: GPL-3.0-only
// C ABI of the Rust crate (../../rust/src/lib.rs).
#pragma once
#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    int32_t status;        // 0 ok; 1 not Wi-Fi, 2 no SSID, 3 SSID too long,
                           // 4 enterprise, 5 unknown security, 6 bad password, 7 no password
    int32_t security;      // 0 open, 1 WEP, 2 WPA/WPA2, 3 WPA3 (SAE)
    int32_t hidden;
    int32_t wep_key_type;  // WEP: 1 key, 2 passphrase
    uint32_t ssid_len;
    uint8_t ssid[128];
    uint32_t password_len;
    uint8_t password[512];
    uint32_t detail_len;
    uint8_t detail[64];
} Fp6WifiQrNetwork;

int32_t fp6_wifiqr_parse(const uint8_t *text, size_t len, Fp6WifiQrNetwork *out);
ssize_t fp6_wifiqr_encode(const uint8_t *ssid, size_t ssid_len, int32_t security,
                          const uint8_t *password, size_t password_len, int32_t hidden,
                          uint8_t *out, size_t cap);
int32_t fp6_wifiqr_matrix(const uint8_t *text, size_t len, uint8_t *out, size_t cap);
ssize_t fp6_wifiqr_decode_luma(const uint8_t *luma, uint32_t width, uint32_t height,
                               uint32_t stride, uint8_t *out, size_t cap);
void fp6_wifiqr_wipe(uint8_t *p, size_t len);

#ifdef __cplusplus
}

#include <QByteArray>
#include <QString>

namespace fp6qr {

// Overwrites a QByteArray's (detached) buffer.
inline void wipe(QByteArray &b)
{
    if (!b.isEmpty())
        fp6_wifiqr_wipe(reinterpret_cast<uint8_t *>(b.data()), size_t(b.size()));
    b.clear();
}

// Overwrites a QString's (detached) buffer. Copies held elsewhere (QML)
// are not reached.
inline void wipe(QString &s)
{
    if (!s.isEmpty())
        fp6_wifiqr_wipe(reinterpret_cast<uint8_t *>(s.data()), size_t(s.size()) * sizeof(QChar));
    s.clear();
}

// security: 0 open, 1 WEP, 2 WPA, 3 SAE
inline QString encode(const QString &ssid, int security, const QString &password, bool hidden)
{
    QByteArray s = ssid.toUtf8(), p = password.toUtf8(), out(1024, '\0');
    ssize_t n = fp6_wifiqr_encode(reinterpret_cast<const uint8_t *>(s.constData()), size_t(s.size()), security,
                                  reinterpret_cast<const uint8_t *>(p.constData()), size_t(p.size()), hidden,
                                  reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()));
    QString r = n > 0 && n <= out.size() ? QString::fromUtf8(out.constData(), int(n)) : QString();
    wipe(p);
    wipe(out);
    return r;
}

} // namespace fp6qr
#endif
