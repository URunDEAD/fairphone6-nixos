// SPDX-License-Identifier: GPL-3.0-only
// Small synchronous NetworkManager D-Bus helpers (the same calls the stock
// Wi-Fi settings plugin makes). Secrets are only read by the callers that
// need them (GetSecrets), never logged.
#pragma once
#include <QDBusConnection>
#include <QDBusObjectPath>
#include <QMap>
#include <QStringList>
#include <QVariantMap>

namespace nm {

typedef QMap<QString, QVariantMap> Settings;

extern const char *const Service;
extern const char *const Path;
extern const char *const Iface;
extern const char *const SettingsPath;
extern const char *const SettingsIface;
extern const char *const ConnectionIface;
extern const char *const ActiveIface;
extern const char *const DeviceIface;

void registerTypes();
QDBusConnection bus();

// First Wi-Fi device (DeviceType 2), or "".
QString wifiDevice();
// Settings path of the connection active on the Wi-Fi device, or "".
QString activeWifiConnection();
QStringList listConnections();
bool getSettings(const QString &path, Settings *out, QString *error = nullptr);
bool getSecrets(const QString &path, const QString &setting, Settings *out, QString *error = nullptr);

bool isWifi(const Settings &s);
QByteArray ssidOf(const Settings &s);
// "open", "wep", "wpa", "sae", "owe" (enhanced open), "enterprise"
QString securityOf(const Settings &s);
// The secrets setting name for a security type ("802-11-wireless-security"
// or "802-1x"), "" for none.
QString secretsSettingFor(const QString &security);
// Connection settings paths whose SSID is `ssid` (infrastructure mode).
QStringList connectionsForSsid(const QByteArray &ssid);

} // namespace nm
