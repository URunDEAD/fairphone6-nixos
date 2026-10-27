// SPDX-License-Identifier: GPL-3.0-only
#include "wifijoin.h"
#include "../common/fp6_wifi_qr.h"

#include <QDBusInterface>
#include <QDBusMessage>
#include <QDBusObjectPath>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusReply>
#include <QDBusVariant>
#include <QUuid>
#include <cstring>

// NMActiveConnectionState
static const uint kActivated = 2;
static const uint kDeactivated = 4;

WifiJoin::WifiJoin(QObject *parent)
    : QObject(parent)
    , m_fake(qEnvironmentVariableIntValue("FP6_WIFIQR_FAKE") == 1)
{
    nm::registerTypes();
    m_timeout.setSingleShot(true);
    m_timeout.setInterval(60000);
    connect(&m_timeout, &QTimer::timeout, this, [this] {
        if (m_state != QLatin1String("connecting"))
            return;
        if (!m_activePath.isEmpty() && !m_fake) {
            QDBusMessage m = QDBusMessage::createMethodCall(nm::Service, nm::Path, nm::Iface, QStringLiteral("DeactivateConnection"));
            m << QVariant::fromValue(QDBusObjectPath(m_activePath));
            nm::bus().call(m, QDBus::Block, 5000);
        }
        failed(tr("“%1” did not answer. Move closer to the router and try again.").arg(m_ssid));
    });
}

WifiJoin::~WifiJoin()
{
    // An activation still running when the app closes goes on in
    // NetworkManager; an unsaved (in-memory) connection simply is not kept.
    wipePending();
}

QString WifiJoin::security() const
{
    switch (m_security) {
    case 1: return QStringLiteral("wep");
    case 2: return QStringLiteral("wpa");
    case 3: return QStringLiteral("sae");
    default: return QStringLiteral("open");
    }
}

QString WifiJoin::securityText() const
{
    switch (m_security) {
    case 1: return tr("WEP");
    case 2: return tr("WPA/WPA2 Personal");
    case 3: return tr("WPA3 Personal");
    default: return tr("Open network (no password)");
    }
}

void WifiJoin::setState(const QString &s, const QString &msg)
{
    m_state = s;
    m_message = msg;
    Q_EMIT stateChanged();
}

void WifiJoin::wipePending()
{
    fp6qr::wipe(m_password);
}

QVariantMap WifiJoin::parse(const QString &text)
{
    QVariantMap r;
    if (m_state == QLatin1String("connecting"))
        return r;
    Fp6WifiQrNetwork n;
    std::memset(&n, 0, sizeof n);
    QByteArray t = text.toUtf8();
    const int status = fp6_wifiqr_parse(reinterpret_cast<const uint8_t *>(t.constData()), size_t(t.size()), &n);
    fp6qr::wipe(t);
    const QString ssid = QString::fromUtf8(reinterpret_cast<const char *>(n.ssid), int(n.ssid_len));
    const QString detail = QString::fromUtf8(reinterpret_cast<const char *>(n.detail), int(n.detail_len));
    r["ssid"] = ssid;
    r["enterprise"] = status == 4;
    switch (status) {
    case 0:
        r["status"] = QStringLiteral("ok");
        break;
    case 1:
        r["status"] = QStringLiteral("not-wifi");
        r["reason"] = tr("This QR code isn't a Wi-Fi network.");
        break;
    case 4:
        r["status"] = QStringLiteral("unsupported");
        r["reason"] = detail.isEmpty()
            ? tr("“%1” uses enterprise (802.1X) sign-in, which can't be set up from a QR code. Add it in Wi-Fi settings.").arg(ssid)
            : tr("“%1” uses enterprise (802.1X, %2) sign-in, which can't be set up from a QR code. Add it in Wi-Fi settings.").arg(ssid, detail);
        break;
    case 5:
        r["status"] = QStringLiteral("unsupported");
        r["reason"] = tr("“%1” uses a security type this phone doesn't support (%2).").arg(ssid, detail);
        break;
    case 6:
        r["status"] = QStringLiteral("invalid");
        r["reason"] = tr("The password in this QR code isn't valid for “%1”.").arg(ssid);
        break;
    case 7:
        r["status"] = QStringLiteral("invalid");
        r["reason"] = tr("This QR code has no password for the secured network “%1”.").arg(ssid);
        break;
    default:
        r["status"] = QStringLiteral("invalid");
        r["reason"] = tr("This Wi-Fi QR code has no valid network name.");
        break;
    }
    if (status == 0) {
        wipePending();
        m_ssid = ssid;
        m_security = n.security;
        m_wepKeyType = n.wep_key_type;
        m_hidden = n.hidden != 0;
        m_password = QString::fromUtf8(reinterpret_cast<const char *>(n.password), int(n.password_len));
        m_existing.clear();
        if (!m_fake) {
            const QStringList same = nm::connectionsForSsid(ssid.toUtf8());
            if (!same.isEmpty())
                m_existing = same.first();
        } else if (qEnvironmentVariableIntValue("FP6_WIFIQR_FAKE_SAVED") == 1) {
            m_existing = QStringLiteral("/fake/1");
        }
        Q_EMIT pendingChanged();
        setState(QStringLiteral("ready"));
    }
    fp6_wifiqr_wipe(n.password, sizeof n.password);
    return r;
}

void WifiJoin::cancel()
{
    if (m_state == QLatin1String("connecting"))
        return;
    wipePending();
    m_ssid.clear();
    m_existing.clear();
    Q_EMIT pendingChanged();
    setState(QStringLiteral("idle"));
}

nm::Settings WifiJoin::buildNew() const
{
    nm::Settings s;
    QVariantMap conn{{"id", m_ssid},
                     {"uuid", QUuid::createUuid().toString(QUuid::WithoutBraces)},
                     {"type", QStringLiteral("802-11-wireless")},
                     {"autoconnect", true}};
    QVariantMap wifi{{"ssid", m_ssid.toUtf8()}, {"mode", QStringLiteral("infrastructure")}};
    if (m_hidden)
        wifi["hidden"] = true;
    const QString mac = qEnvironmentVariable("FP6_WIFIQR_ASSIGNED_MAC");
    if (!mac.isEmpty())
        wifi["assigned-mac-address"] = mac;
    if (m_security != 0) {
        wifi["security"] = QStringLiteral("802-11-wireless-security");
        QVariantMap sec;
        switch (m_security) {
        case 1:
            sec["key-mgmt"] = QStringLiteral("none");
            sec["auth-alg"] = QStringLiteral("open");
            sec["wep-key0"] = m_password;
            sec["wep-key-type"] = uint(m_wepKeyType == 1 ? 1 : 2);
            break;
        case 3:
            sec["key-mgmt"] = QStringLiteral("sae");
            sec["psk"] = m_password;
            break;
        default:
            sec["key-mgmt"] = QStringLiteral("wpa-psk");
            sec["psk"] = m_password;
            break;
        }
        s["802-11-wireless-security"] = sec;
    }
    s["connection"] = conn;
    s["802-11-wireless"] = wifi;
    s["ipv4"] = QVariantMap{{"method", QStringLiteral("auto")}};
    s["ipv6"] = QVariantMap{{"method", QStringLiteral("auto")}};
    return s;
}

void WifiJoin::join()
{
    if (m_state != QLatin1String("ready") && m_state != QLatin1String("failed"))
        return;
    if (m_ssid.isEmpty())
        return;
    setState(QStringLiteral("connecting"), tr("Connecting to “%1”…").arg(m_ssid));
    m_connPath.clear();
    m_activePath.clear();
    m_created = false;
    m_timeout.start();
    if (m_fake) {
        // previews: FP6_WIFIQR_FAKE_JOIN_MS delay, FP6_WIFIQR_FAKE_JOIN=fail
        const int ms = qEnvironmentVariableIsSet("FP6_WIFIQR_FAKE_JOIN_MS") ? qEnvironmentVariableIntValue("FP6_WIFIQR_FAKE_JOIN_MS") : 1500;
        const bool fail = qEnvironmentVariable("FP6_WIFIQR_FAKE_JOIN") == QLatin1String("fail");
        QTimer::singleShot(ms, this, [this, fail] { if (fail) failed(reasonText(9)); else succeeded(); });
        return;
    }

    // Wi-Fi off: turn it on (as Android does when joining from a QR code).
    QDBusInterface props(nm::Service, nm::Path, QStringLiteral("org.freedesktop.DBus.Properties"), nm::bus());
    props.setTimeout(5000);
    QDBusReply<QDBusVariant> en = props.call(QStringLiteral("Get"), QString::fromLatin1(nm::Iface), QStringLiteral("WirelessEnabled"));
    if (en.isValid() && !en.value().variant().toBool())
        props.call(QStringLiteral("Set"), QString::fromLatin1(nm::Iface), QStringLiteral("WirelessEnabled"),
                   QVariant::fromValue(QDBusVariant(true)));

    const QString dev = nm::wifiDevice();
    if (dev.isEmpty()) {
        failed(tr("This phone has no Wi-Fi device available."));
        return;
    }

    if (m_existing.isEmpty()) {
        activateNew(dev, buildNew());
        return;
    }
    // A saved network with this SSID: activate it if nothing changed.
    nm::Settings old, secrets;
    if (!nm::getSettings(m_existing, &old)) {
        m_existing.clear();
        activateNew(dev, buildNew());
        return;
    }
    const QString oldSec = nm::securityOf(old);
    QString oldPw;
    const QString setting = nm::secretsSettingFor(oldSec);
    if (!setting.isEmpty() && nm::getSecrets(m_existing, setting, &secrets)) {
        const QVariantMap s = secrets.value(setting);
        oldPw = oldSec == QLatin1String("wep") ? s.value("wep-key0").toString() : s.value("psk").toString();
    }
    const bool oldHidden = old.value("802-11-wireless").value("hidden").toBool();
    const bool same = oldSec == security() && oldPw == m_password && oldHidden == m_hidden;
    fp6qr::wipe(oldPw);
    if (same) {
        activate(dev, m_existing);
        return;
    }
    // Changed password/security: a new connection that keeps the saved one's
    // private address (stable-id feeds the "stable" MAC; an explicit or
    // pinned MAC is copied), the old one goes once the new one works.
    nm::Settings s = buildNew();
    const QVariantMap oc = old.value("connection");
    const QString stableId = oc.value("stable-id").toString();
    s["connection"]["stable-id"] = stableId.isEmpty() ? oc.value("uuid").toString() : stableId;
    const QString oldMac = old.value("802-11-wireless").value("assigned-mac-address").toString();
    if (!oldMac.isEmpty())
        s["802-11-wireless"]["assigned-mac-address"] = oldMac;
    else
        s["802-11-wireless"].remove("assigned-mac-address");
    activateNew(dev, s);
}

void WifiJoin::activateNew(const QString &device, const nm::Settings &settings)
{
    QDBusMessage m = QDBusMessage::createMethodCall(nm::Service, nm::Path, nm::Iface, QStringLiteral("AddAndActivateConnection2"));
    m << QVariant::fromValue(settings) << QVariant::fromValue(QDBusObjectPath(device))
      << QVariant::fromValue(QDBusObjectPath("/")) << QVariantMap{{"persist", QStringLiteral("memory")}};
    auto *w = new QDBusPendingCallWatcher(nm::bus().asyncCall(m, 30000), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, w, device, settings] {
        QDBusPendingReply<QDBusObjectPath, QDBusObjectPath, QVariantMap> r = *w;
        w->deleteLater();
        if (r.isError()) {
            if (r.error().type() == QDBusError::UnknownMethod) {
                // NetworkManager < 1.16: saved right away, deleted on failure.
                QDBusMessage m1 = QDBusMessage::createMethodCall(nm::Service, nm::Path, nm::Iface, QStringLiteral("AddAndActivateConnection"));
                m1 << QVariant::fromValue(settings) << QVariant::fromValue(QDBusObjectPath(device)) << QVariant::fromValue(QDBusObjectPath("/"));
                QDBusPendingReply<QDBusObjectPath, QDBusObjectPath> r1 = nm::bus().call(m1, QDBus::Block, 30000);
                if (!r1.isError()) {
                    m_connPath = r1.argumentAt<0>().path();
                    m_created = true;
                    watchActive(r1.argumentAt<1>().path());
                    return;
                }
            }
            qWarning("fp6-wifi-qr-scan: AddAndActivateConnection: %s", qPrintable(r.error().name()));
            failed(tr("Couldn't add “%1”: %2").arg(m_ssid, r.error().message()));
            return;
        }
        m_connPath = r.argumentAt<0>().path();
        m_created = true;
        watchActive(r.argumentAt<1>().path());
    });
}

void WifiJoin::activate(const QString &device, const QString &path)
{
    QDBusMessage m = QDBusMessage::createMethodCall(nm::Service, nm::Path, nm::Iface, QStringLiteral("ActivateConnection"));
    m << QVariant::fromValue(QDBusObjectPath(path)) << QVariant::fromValue(QDBusObjectPath(device))
      << QVariant::fromValue(QDBusObjectPath("/"));
    auto *w = new QDBusPendingCallWatcher(nm::bus().asyncCall(m, 30000), this);
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, w, path] {
        QDBusPendingReply<QDBusObjectPath> r = *w;
        w->deleteLater();
        if (r.isError()) {
            failed(tr("Couldn't connect to “%1”: %2").arg(m_ssid, r.error().message()));
            return;
        }
        m_connPath = path;
        watchActive(r.value().path());
    });
}

void WifiJoin::watchActive(const QString &active)
{
    m_activePath = active;
    nm::bus().connect(nm::Service, active, nm::ActiveIface, QStringLiteral("StateChanged"),
                      this, SLOT(onActiveStateChanged(uint, uint)));
    QDBusInterface props(nm::Service, active, QStringLiteral("org.freedesktop.DBus.Properties"), nm::bus());
    QDBusReply<QDBusVariant> st = props.call(QStringLiteral("Get"), QString::fromLatin1(nm::ActiveIface), QStringLiteral("State"));
    if (st.isValid() && st.value().variant().toUInt() == kActivated)
        succeeded();
}

void WifiJoin::onActiveStateChanged(uint state, uint reason)
{
    if (m_state != QLatin1String("connecting"))
        return;
    if (state == kActivated)
        succeeded();
    else if (state == kDeactivated)
        failed(reasonText(reason));
}

QString WifiJoin::reasonText(uint reason) const
{
    switch (reason) {
    case 9:   // NO_SECRETS
    case 10:  // LOGIN_FAILED
        return tr("“%1” didn't accept the password from the QR code.").arg(m_ssid);
    case 6:   // CONNECT_TIMEOUT
        return tr("“%1” did not answer. Move closer to the router and try again.").arg(m_ssid);
    case 2:   // USER_DISCONNECTED
    case 11:  // CONNECTION_REMOVED
        return tr("Connecting to “%1” was cancelled.").arg(m_ssid);
    default:
        return tr("Couldn't connect to “%1”.").arg(m_ssid);
    }
}

void WifiJoin::succeeded()
{
    if (m_state != QLatin1String("connecting"))
        return;
    m_timeout.stop();
    if (!m_fake) {
        nm::bus().disconnect(nm::Service, m_activePath, nm::ActiveIface, QStringLiteral("StateChanged"),
                             this, SLOT(onActiveStateChanged(uint, uint)));
        if (m_created) {
            // persist=memory -> keyfile now; then the replaced network goes.
            QDBusMessage save = QDBusMessage::createMethodCall(nm::Service, m_connPath, nm::ConnectionIface, QStringLiteral("Save"));
            QDBusMessage sr = nm::bus().call(save, QDBus::Block, 10000);
            if (sr.type() == QDBusMessage::ErrorMessage && sr.errorName() != QLatin1String("org.freedesktop.DBus.Error.UnknownMethod"))
                qWarning("fp6-wifi-qr-scan: Save: %s", qPrintable(sr.errorName()));
            if (!m_existing.isEmpty() && m_existing != m_connPath) {
                QDBusMessage del = QDBusMessage::createMethodCall(nm::Service, m_existing, nm::ConnectionIface, QStringLiteral("Delete"));
                nm::bus().call(del, QDBus::Block, 10000);
            }
        }
    }
    wipePending();
    setState(QStringLiteral("connected"), tr("Connected to “%1”").arg(m_ssid));
}

void WifiJoin::failed(const QString &msg)
{
    m_timeout.stop();
    if (!m_fake && !m_activePath.isEmpty())
        nm::bus().disconnect(nm::Service, m_activePath, nm::ActiveIface, QStringLiteral("StateChanged"),
                             this, SLOT(onActiveStateChanged(uint, uint)));
    if (!m_fake && m_created && !m_connPath.isEmpty()) {
        // the connection added for this attempt (in memory) goes again
        QDBusMessage del = QDBusMessage::createMethodCall(nm::Service, m_connPath, nm::ConnectionIface, QStringLiteral("Delete"));
        nm::bus().call(del, QDBus::Block, 10000);
    }
    m_created = false;
    m_connPath.clear();
    m_activePath.clear();
    // the pending network stays for "Try again"
    setState(QStringLiteral("failed"), msg);
}
