// SPDX-License-Identifier: GPL-3.0-only
// WifiShare: one saved Wi-Fi connection for Settings > Wi-Fi > network
// details: what it is (no secrets), the phone's lock type, the passcode
// check (PAM, service fp6-wifi-qr) / fingerprint result, and - only after a
// successful check, or with no screen lock - the password from
// NetworkManager (GetSecrets) and the QR payload. Nothing is logged.
#pragma once
#include <QObject>
#include <QTimer>

class WifiShare : public QObject
{
    Q_OBJECT
    // in
    Q_PROPERTY(QString ssid READ ssid WRITE setSsid NOTIFY ssidChanged)
    // NetworkManager settings path; "" = the active Wi-Fi connection
    // (with `ssid`) or the first saved one with that SSID.
    Q_PROPERTY(QString connectionPath READ connectionPath WRITE setConnectionPath NOTIFY connectionPathChanged)
    // the connection (no secrets)
    Q_PROPERTY(bool found READ found NOTIFY infoChanged)
    Q_PROPERTY(QString security READ security NOTIFY infoChanged)   // open wep wpa sae owe enterprise
    Q_PROPERTY(QString securityText READ securityText NOTIFY infoChanged)
    Q_PROPERTY(bool hidden READ hidden NOTIFY infoChanged)
    Q_PROPERTY(bool shareable READ shareable NOTIFY infoChanged)
    Q_PROPERTY(bool hasPassword READ hasPassword NOTIFY infoChanged)
    // screen lock
    Q_PROPERTY(QString lockType READ lockType NOTIFY lockChanged)     // swipe passcode passphrase
    Q_PROPERTY(bool fingerprintEnabled READ fingerprintEnabled NOTIFY lockChanged)
    Q_PROPERTY(bool needsAuth READ needsAuth NOTIFY authChanged)
    Q_PROPERTY(bool authenticated READ authenticated NOTIFY authChanged)
    Q_PROPERTY(bool checking READ checking NOTIFY checkingChanged)
    Q_PROPERTY(int retryAfter READ retryAfter NOTIFY retryAfterChanged)
    Q_PROPERTY(int uid READ uid CONSTANT)
    // after authentication
    Q_PROPERTY(bool revealed READ revealed NOTIFY revealedChanged)
    Q_PROPERTY(QString password READ password NOTIFY revealedChanged)
    Q_PROPERTY(QString payload READ payload NOTIFY revealedChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
public:
    explicit WifiShare(QObject *parent = nullptr);
    ~WifiShare() override;

    QString ssid() const { return m_ssid; }
    void setSsid(const QString &s);
    QString connectionPath() const { return m_path; }
    void setConnectionPath(const QString &p);
    bool found() const { return m_found; }
    QString security() const { return m_security; }
    QString securityText() const;
    bool hidden() const { return m_hidden; }
    bool shareable() const;
    bool hasPassword() const;
    QString lockType() const { return m_lockType; }
    bool fingerprintEnabled() const { return m_fingerprint; }
    bool needsAuth() const;
    bool authenticated() const { return m_authenticated; }
    bool checking() const { return m_checking; }
    int retryAfter() const { return m_retryAfter; }
    int uid() const;
    bool revealed() const { return m_revealed; }
    QString password() const { return m_password; }
    QString payload() const { return m_payload; }
    QString error() const { return m_error; }

    // Re-reads the connection and the lock settings.
    Q_INVOKABLE void refresh();
    // Asynchronous; emits passcodeChecked(ok, message).
    Q_INVOKABLE void checkPasscode(const QString &code);
    // From the Biometryd identify operation (QML): the matched uid.
    Q_INVOKABLE void fingerprintIdentified(int uid);
    // Fetches the password and builds the payload; only when !needsAuth.
    Q_INVOKABLE void reveal();
    // Forgets the password, payload and authentication.
    Q_INVOKABLE void clear();

Q_SIGNALS:
    void ssidChanged();
    void connectionPathChanged();
    void infoChanged();
    void lockChanged();
    void authChanged();
    void checkingChanged();
    void retryAfterChanged();
    void revealedChanged();
    void errorChanged();
    void passcodeChecked(bool ok, const QString &message);

private:
    void setError(const QString &e);
    void setAuthenticated();
    void failedAttempt();
    void readLock();
    void finishReveal(QString password);

    QString m_ssid, m_path, m_resolvedPath, m_security;
    bool m_found = false, m_hidden = false;
    QString m_lockType = QStringLiteral("passphrase");
    bool m_fingerprint = false;
    bool m_authenticated = false, m_checking = false, m_revealed = false;
    int m_failures = 0, m_retryAfter = 0;
    QTimer m_retryTimer;
    QString m_password, m_payload, m_error;
    bool m_fake = false;
    quint64 m_generation = 0;
};
