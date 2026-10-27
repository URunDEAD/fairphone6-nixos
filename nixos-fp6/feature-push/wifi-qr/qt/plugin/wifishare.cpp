// SPDX-License-Identifier: GPL-3.0-only
#include "wifishare.h"
#include "../common/fp6_wifi_qr.h"
#include "../common/nmutil.h"

#include <QDBusInterface>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusReply>
#include <QDBusVariant>
#include <QFutureWatcher>
#include <QPointer>
#include <QtConcurrent/QtConcurrentRun>

#include <security/pam_appl.h>
#include <pwd.h>
#include <unistd.h>
#include <algorithm>
#include <cstdlib>
#include <cstring>

// PAM service (/etc/pam.d/fp6-wifi-qr: pam_unix auth + account). The phone
// passcode/passphrase is the account password (as for the greeter).
static const char *const kPamService = "fp6-wifi-qr";
static const char *const kFakePasscode = "1234";       // FP6_WIFIQR_FAKE previews only
static const char *const kFakePassword = "p@ss:word,1";
static const int kFreeAttempts = 5;

namespace {

struct PamCtx { const char *code; };

int conv(int n, const struct pam_message **msg, struct pam_response **resp, void *data)
{
    auto *ctx = static_cast<PamCtx *>(data);
    auto *r = static_cast<pam_response *>(calloc(size_t(n), sizeof(pam_response)));
    if (!r)
        return PAM_BUF_ERR;
    for (int i = 0; i < n; ++i) {
        if (msg[i]->msg_style == PAM_PROMPT_ECHO_OFF || msg[i]->msg_style == PAM_PROMPT_ECHO_ON)
            r[i].resp = strdup(ctx->code);
    }
    *resp = r;
    return PAM_SUCCESS;
}

// Runs in a worker thread. Returns the PAM result.
int pamCheck(QByteArray user, QByteArray code)
{
    PamCtx ctx{code.constData()};
    struct pam_conv c{conv, &ctx};
    pam_handle_t *h = nullptr;
    int r = pam_start(kPamService, user.constData(), &c, &h);
    if (r == PAM_SUCCESS)
        r = pam_authenticate(h, PAM_DISALLOW_NULL_AUTHTOK);
    if (r == PAM_SUCCESS)
        r = pam_acct_mgmt(h, PAM_DISALLOW_NULL_AUTHTOK);
    if (h)
        pam_end(h, r);
    fp6qr::wipe(code);
    return r;
}

QByteArray userName()
{
    struct passwd *pw = getpwuid(getuid());
    return pw ? QByteArray(pw->pw_name) : qgetenv("USER");
}

int securityCode(const QString &s)
{
    if (s == QLatin1String("wep")) return 1;
    if (s == QLatin1String("wpa")) return 2;
    if (s == QLatin1String("sae")) return 3;
    return 0;  // open, owe
}

} // namespace

WifiShare::WifiShare(QObject *parent)
    : QObject(parent)
    , m_fake(qEnvironmentVariableIntValue("FP6_WIFIQR_FAKE") == 1)
{
    nm::registerTypes();
    m_retryTimer.setInterval(1000);
    connect(&m_retryTimer, &QTimer::timeout, this, [this] {
        if (--m_retryAfter <= 0) {
            m_retryAfter = 0;
            m_retryTimer.stop();
        }
        Q_EMIT retryAfterChanged();
    });
    readLock();
}

WifiShare::~WifiShare()
{
    fp6qr::wipe(m_password);
    fp6qr::wipe(m_payload);
}

void WifiShare::setSsid(const QString &s)
{
    if (s == m_ssid)
        return;
    m_ssid = s;
    Q_EMIT ssidChanged();
    clear();
    refresh();
}

void WifiShare::setConnectionPath(const QString &p)
{
    if (p == m_path)
        return;
    m_path = p;
    Q_EMIT connectionPathChanged();
    clear();
    refresh();
}

QString WifiShare::securityText() const
{
    if (m_security == QLatin1String("open")) return tr("None");
    if (m_security == QLatin1String("owe")) return tr("Enhanced Open");
    if (m_security == QLatin1String("wep")) return tr("WEP");
    if (m_security == QLatin1String("wpa")) return tr("WPA/WPA2 Personal");
    if (m_security == QLatin1String("sae")) return tr("WPA3 Personal");
    if (m_security == QLatin1String("enterprise")) return tr("Enterprise (802.1X)");
    return QString();
}

bool WifiShare::shareable() const
{
    return m_found && m_security != QLatin1String("enterprise") && !m_security.isEmpty();
}

bool WifiShare::hasPassword() const
{
    return m_found && m_security != QLatin1String("open") && m_security != QLatin1String("owe");
}

bool WifiShare::needsAuth() const
{
    return hasPassword() && !m_authenticated && m_lockType != QLatin1String("swipe");
}

int WifiShare::uid() const
{
    return int(getuid());
}

void WifiShare::setError(const QString &e)
{
    if (e == m_error)
        return;
    m_error = e;
    Q_EMIT errorChanged();
}

void WifiShare::readLock()
{
    QString type = QStringLiteral("passphrase");
    bool fp = false;
    if (m_fake) {
        type = qEnvironmentVariable("FP6_WIFIQR_FAKE_LOCK", QStringLiteral("passcode"));
        fp = qEnvironmentVariableIntValue("FP6_WIFIQR_FAKE_FINGERPRINT") == 1;
    } else {
        // Same sources as Settings > Security & Privacy (securityprivacy.cpp):
        // PasswordMode 2 (none) = swipe; PasswordDisplayHint 1 = passcode.
        const QString path = QStringLiteral("/org/freedesktop/Accounts/User%1").arg(getuid());
        QDBusInterface props(QStringLiteral("org.freedesktop.Accounts"), path,
                             QStringLiteral("org.freedesktop.DBus.Properties"), QDBusConnection::systemBus());
        props.setTimeout(3000);
        QDBusReply<QDBusVariant> mode = props.call(QStringLiteral("Get"), QStringLiteral("org.freedesktop.Accounts.User"),
                                                   QStringLiteral("PasswordMode"));
        if (mode.isValid() && mode.value().variant().toInt() == 2) {
            type = QStringLiteral("swipe");
        } else {
            QDBusReply<QDBusVariant> hint = props.call(QStringLiteral("Get"), QStringLiteral("com.lomiri.AccountsService.SecurityPrivacy"),
                                                       QStringLiteral("PasswordDisplayHint"));
            if (hint.isValid() && hint.value().variant().toInt() == 1)
                type = QStringLiteral("passcode");
        }
        QDBusReply<QDBusVariant> f = props.call(QStringLiteral("Get"), QStringLiteral("com.lomiri.AccountsService.SecurityPrivacy"),
                                                QStringLiteral("EnableFingerprintIdentification"));
        fp = f.isValid() && f.value().variant().toBool();
    }
    // Swipe never has fingerprint unlock (Settings turns it off).
    if (type == QLatin1String("swipe"))
        fp = false;
    if (type != m_lockType || fp != m_fingerprint) {
        m_lockType = type;
        m_fingerprint = fp;
        Q_EMIT lockChanged();
        Q_EMIT authChanged();
    }
}

void WifiShare::refresh()
{
    readLock();
    bool found = false, hidden = false;
    QString security, resolved;
    if (m_fake) {
        found = !m_ssid.isEmpty() || !m_path.isEmpty();
        security = qEnvironmentVariable("FP6_WIFIQR_FAKE_SECURITY", QStringLiteral("wpa"));
        hidden = qEnvironmentVariableIntValue("FP6_WIFIQR_FAKE_HIDDEN") == 1;
        if (m_ssid.isEmpty())
            m_ssid = QStringLiteral("Test;Net");
    } else {
        resolved = m_path;
        const QByteArray want = m_ssid.toUtf8();
        nm::Settings s;
        if (resolved.isEmpty()) {
            const QString active = nm::activeWifiConnection();
            if (!active.isEmpty() && nm::getSettings(active, &s) && (want.isEmpty() || nm::ssidOf(s) == want))
                resolved = active;
            else if (!want.isEmpty()) {
                const QStringList all = nm::connectionsForSsid(want);
                if (!all.isEmpty())
                    resolved = all.first();
            }
            s.clear();
        }
        QString err;
        if (!resolved.isEmpty() && nm::getSettings(resolved, &s, &err) && nm::isWifi(s)) {
            found = true;
            security = nm::securityOf(s);
            hidden = s.value("802-11-wireless").value("hidden").toBool();
            if (m_ssid.isEmpty())
                m_ssid = QString::fromUtf8(nm::ssidOf(s));
        }
    }
    m_resolvedPath = resolved;
    if (found != m_found || security != m_security || hidden != m_hidden) {
        m_found = found;
        m_security = security;
        m_hidden = hidden;
        Q_EMIT infoChanged();
        Q_EMIT authChanged();
    }
}

void WifiShare::checkPasscode(const QString &code)
{
    if (m_checking || m_retryAfter > 0 || code.isEmpty())
        return;
    m_checking = true;
    Q_EMIT checkingChanged();
    const quint64 gen = m_generation;
    if (m_fake) {
        const bool ok = code == QLatin1String(kFakePasscode);
        QTimer::singleShot(400, this, [this, ok, gen] {
            m_checking = false;
            Q_EMIT checkingChanged();
            if (gen != m_generation)
                return;
            if (ok) setAuthenticated(); else failedAttempt();
        });
        return;
    }
    auto *w = new QFutureWatcher<int>(this);
    connect(w, &QFutureWatcher<int>::finished, this, [this, w, gen] {
        const int r = w->result();
        w->deleteLater();
        m_checking = false;
        Q_EMIT checkingChanged();
        if (gen != m_generation)
            return;
        if (r == PAM_SUCCESS) setAuthenticated(); else failedAttempt();
    });
    QByteArray c = code.toUtf8();
    w->setFuture(QtConcurrent::run(pamCheck, userName(), c));
    fp6qr::wipe(c);
}

void WifiShare::failedAttempt()
{
    ++m_failures;
    QString msg = m_lockType == QLatin1String("passcode") ? tr("Incorrect passcode. Try again.")
                                                          : tr("Incorrect passphrase. Try again.");
    if (m_failures >= kFreeAttempts) {
        // 30 s after 5 wrong tries, doubling up to 10 min.
        m_retryAfter = std::min(600, 30 << std::min(5, m_failures - kFreeAttempts));
        m_retryTimer.start();
        Q_EMIT retryAfterChanged();
        msg = tr("Too many attempts. Try again in %n seconds.", nullptr, m_retryAfter);
    }
    Q_EMIT passcodeChecked(false, msg);
}

void WifiShare::setAuthenticated()
{
    m_failures = 0;
    m_authenticated = true;
    Q_EMIT authChanged();
    Q_EMIT passcodeChecked(true, QString());
    reveal();
}

void WifiShare::fingerprintIdentified(int matchedUid)
{
    if (!m_fingerprint || m_retryAfter > 0)
        return;
    if (matchedUid == int(getuid()))
        setAuthenticated();
}

void WifiShare::reveal()
{
    if (!m_found || needsAuth())
        return;
    setError(QString());
    if (!hasPassword()) {
        finishReveal(QString());
        return;
    }
    if (m_fake) {
        finishReveal(QString::fromLatin1(kFakePassword));
        return;
    }
    const QString setting = nm::secretsSettingFor(m_security);
    QDBusMessage m = QDBusMessage::createMethodCall(nm::Service, m_resolvedPath, nm::ConnectionIface, QStringLiteral("GetSecrets"));
    m << setting;
    auto *w = new QDBusPendingCallWatcher(nm::bus().asyncCall(m, 15000), this);
    const quint64 gen = m_generation;
    const QString security = m_security;
    connect(w, &QDBusPendingCallWatcher::finished, this, [this, w, gen, setting, security] {
        QDBusPendingReply<nm::Settings> r = *w;
        w->deleteLater();
        if (gen != m_generation)
            return;
        if (r.isError()) {
            setError(tr("The password could not be read from NetworkManager."));
            return;
        }
        nm::Settings s = r.value();
        QVariantMap sec = s.value(setting);
        QString pw;
        if (security == QLatin1String("wep")) {
            const int idx = sec.value("wep-tx-keyidx").toInt();
            pw = sec.value(QStringLiteral("wep-key%1").arg(idx)).toString();
            if (pw.isEmpty()) pw = sec.value("wep-key0").toString();
        } else if (security == QLatin1String("enterprise")) {
            pw = sec.value("password").toString();
        } else {
            pw = sec.value("psk").toString();
        }
        if (pw.isEmpty()) {
            setError(tr("No password is saved for this network."));
            return;
        }
        finishReveal(pw);
    });
}

void WifiShare::finishReveal(QString pw)
{
    fp6qr::wipe(m_password);
    fp6qr::wipe(m_payload);
    m_password = pw;
    // Enterprise networks: the password row only, no QR code.
    if (shareable())
        m_payload = fp6qr::encode(m_ssid, securityCode(m_security), pw, m_hidden);
    fp6qr::wipe(pw);
    m_revealed = true;
    Q_EMIT revealedChanged();
}

void WifiShare::clear()
{
    ++m_generation;
    fp6qr::wipe(m_password);
    fp6qr::wipe(m_payload);
    const bool wasRevealed = m_revealed, wasAuth = m_authenticated;
    m_revealed = false;
    m_authenticated = false;
    if (wasRevealed)
        Q_EMIT revealedChanged();
    if (wasAuth)
        Q_EMIT authChanged();
}
