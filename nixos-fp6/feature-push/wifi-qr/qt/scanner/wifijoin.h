// SPDX-License-Identifier: GPL-3.0-only
// WifiJoin: parses a scanned payload (Rust) and joins the network through
// NetworkManager. The password stays in C++ (QML only learns whether there
// is one).
//
// New network: AddAndActivateConnection2 with persist=memory, saved to disk
// only once it is activated (a wrong password leaves nothing behind).
// Saved network with the same SSID: same password -> just activated;
// different -> a new in-memory connection that keeps the saved one's
// stable-id and MAC setting (same private address), activated, saved, and
// the old one removed; on failure the old one is untouched.
// MAC address: a saved network's own setting is kept; new networks get
// FP6_WIFIQR_ASSIGNED_MAC (the privacy-lock default "stable": a private
// per-network address), else NetworkManager's global default.
#pragma once
#include <QObject>
#include <QTimer>
#include <QVariantMap>

#include "../common/nmutil.h"

class WifiJoin : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString state READ state NOTIFY stateChanged)       // idle ready connecting connected failed
    Q_PROPERTY(QString message READ message NOTIFY stateChanged)
    Q_PROPERTY(QString ssid READ ssid NOTIFY pendingChanged)
    Q_PROPERTY(QString security READ security NOTIFY pendingChanged) // open wep wpa sae
    Q_PROPERTY(QString securityText READ securityText NOTIFY pendingChanged)
    Q_PROPERTY(bool hidden READ hidden NOTIFY pendingChanged)
    Q_PROPERTY(bool saved READ saved NOTIFY pendingChanged)         // a saved network has this SSID
public:
    explicit WifiJoin(QObject *parent = nullptr);
    ~WifiJoin() override;

    QString state() const { return m_state; }
    QString message() const { return m_message; }
    QString ssid() const { return m_ssid; }
    QString security() const;
    QString securityText() const;
    bool hidden() const { return m_hidden; }
    bool saved() const { return !m_existing.isEmpty(); }

    // Parses a scanned text. Returns {status: "ok" | "not-wifi" |
    // "unsupported" | "invalid", reason: <text>, ssid, enterprise: bool}.
    // "ok" makes it the pending network (state "ready").
    Q_INVOKABLE QVariantMap parse(const QString &text);
    Q_INVOKABLE void join();
    // Drops the pending network (state "idle").
    Q_INVOKABLE void cancel();

Q_SIGNALS:
    void stateChanged();
    void pendingChanged();

private Q_SLOTS:
    void onActiveStateChanged(uint state, uint reason);

private:
    void setState(const QString &s, const QString &msg = QString());
    nm::Settings buildNew() const;
    void activateNew(const QString &device, const nm::Settings &settings);
    void activate(const QString &device, const QString &path);
    void watchActive(const QString &active);
    void succeeded();
    void failed(const QString &msg);
    void wipePending();
    QString reasonText(uint reason) const;

    QString m_state = QStringLiteral("idle"), m_message;
    QString m_ssid;
    int m_security = 0, m_wepKeyType = 0;
    bool m_hidden = false;
    QString m_password;
    QString m_existing;             // settings path of a saved network with this SSID
    // during an activation
    QString m_connPath, m_activePath;
    bool m_created = false;
    QTimer m_timeout;
    bool m_fake = false;
};
