// SPDX-License-Identifier: GPL-3.0-only
#include "nmutil.h"

#include <QDBusArgument>
#include <QDBusInterface>
#include <QDBusMetaType>
#include <QDBusReply>
#include <QDBusVariant>

namespace nm {

const char *const Service = "org.freedesktop.NetworkManager";
const char *const Path = "/org/freedesktop/NetworkManager";
const char *const Iface = "org.freedesktop.NetworkManager";
const char *const SettingsPath = "/org/freedesktop/NetworkManager/Settings";
const char *const SettingsIface = "org.freedesktop.NetworkManager.Settings";
const char *const ConnectionIface = "org.freedesktop.NetworkManager.Settings.Connection";
const char *const ActiveIface = "org.freedesktop.NetworkManager.Connection.Active";
const char *const DeviceIface = "org.freedesktop.NetworkManager.Device";

void registerTypes()
{
    static bool done = false;
    if (done)
        return;
    done = true;
    qDBusRegisterMetaType<Settings>();
}

QDBusConnection bus()
{
    return QDBusConnection::systemBus();
}

static QVariant property(const QString &path, const char *iface, const char *name)
{
    QDBusInterface props(Service, path, "org.freedesktop.DBus.Properties", bus());
    props.setTimeout(5000);
    QDBusReply<QDBusVariant> r = props.call("Get", QString::fromLatin1(iface), QString::fromLatin1(name));
    return r.isValid() ? r.value().variant() : QVariant();
}

static QList<QDBusObjectPath> pathList(const QDBusMessage &reply)
{
    QList<QDBusObjectPath> out;
    if (reply.type() == QDBusMessage::ReplyMessage && !reply.arguments().isEmpty())
        out = qdbus_cast<QList<QDBusObjectPath>>(reply.arguments().at(0));
    return out;
}

QString wifiDevice()
{
    QDBusInterface mgr(Service, Path, Iface, bus());
    mgr.setTimeout(5000);
    for (const QDBusObjectPath &d : pathList(mgr.call("GetDevices"))) {
        if (property(d.path(), DeviceIface, "DeviceType").toUInt() == 2)
            return d.path();
    }
    return QString();
}

QString activeWifiConnection()
{
    const QString dev = wifiDevice();
    if (dev.isEmpty())
        return QString();
    const QString ac = property(dev, DeviceIface, "ActiveConnection").value<QDBusObjectPath>().path();
    if (ac.isEmpty() || ac == "/")
        return QString();
    const QString c = property(ac, ActiveIface, "Connection").value<QDBusObjectPath>().path();
    return c == "/" ? QString() : c;
}

QStringList listConnections()
{
    QDBusInterface s(Service, SettingsPath, SettingsIface, bus());
    s.setTimeout(5000);
    QStringList out;
    for (const QDBusObjectPath &p : pathList(s.call("ListConnections")))
        out << p.path();
    return out;
}

static bool settingsCall(const QString &path, const char *method, const QVariantList &args, Settings *out, QString *error)
{
    registerTypes();
    QDBusMessage m = QDBusMessage::createMethodCall(Service, path, ConnectionIface, QString::fromLatin1(method));
    m.setArguments(args);
    QDBusReply<Settings> r = bus().call(m, QDBus::Block, 10000);
    if (!r.isValid()) {
        if (error)
            *error = r.error().message();
        return false;
    }
    *out = r.value();
    return true;
}

bool getSettings(const QString &path, Settings *out, QString *error)
{
    return settingsCall(path, "GetSettings", {}, out, error);
}

bool getSecrets(const QString &path, const QString &setting, Settings *out, QString *error)
{
    return settingsCall(path, "GetSecrets", {setting}, out, error);
}

bool isWifi(const Settings &s)
{
    return s.value("connection").value("type").toString() == QLatin1String("802-11-wireless");
}

QByteArray ssidOf(const Settings &s)
{
    return s.value("802-11-wireless").value("ssid").toByteArray();
}

QString securityOf(const Settings &s)
{
    if (s.contains("802-1x"))
        return QStringLiteral("enterprise");
    if (!s.contains("802-11-wireless-security"))
        return QStringLiteral("open");
    const QString km = s.value("802-11-wireless-security").value("key-mgmt").toString();
    if (km == QLatin1String("none"))
        return QStringLiteral("wep");
    if (km == QLatin1String("wpa-psk"))
        return QStringLiteral("wpa");
    if (km == QLatin1String("sae"))
        return QStringLiteral("sae");
    if (km == QLatin1String("owe"))
        return QStringLiteral("owe");
    return QStringLiteral("enterprise");  // wpa-eap, wpa-eap-suite-b-192, ieee8021x
}

QString secretsSettingFor(const QString &security)
{
    if (security == QLatin1String("wep") || security == QLatin1String("wpa") || security == QLatin1String("sae"))
        return QStringLiteral("802-11-wireless-security");
    if (security == QLatin1String("enterprise"))
        return QStringLiteral("802-1x");
    return QString();
}

QStringList connectionsForSsid(const QByteArray &ssid)
{
    QStringList out;
    for (const QString &p : listConnections()) {
        Settings s;
        if (!getSettings(p, &s) || !isWifi(s) || ssidOf(s) != ssid)
            continue;
        const QString mode = s.value("802-11-wireless").value("mode").toString();
        if (mode.isEmpty() || mode == QLatin1String("infrastructure"))
            out << p;
    }
    return out;
}

} // namespace nm
