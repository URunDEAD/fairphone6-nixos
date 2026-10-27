// SPDX-License-Identifier: GPL-3.0-only
// Fp6.WifiQr: QML module for System Settings > Wi-Fi (share / scan).
//   QrCode     crisp QR code item
//   WifiShare  one saved network: lock check, password, payload
//   WifiQr     singleton: scannerAvailable, launchScanner()
#include "../common/qrcodeitem.h"
#include "wifishare.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDir>
#include <QFileInfo>
#include <QProcess>
#include <QQmlEngine>
#include <QQmlExtensionPlugin>
#include <QStandardPaths>

static const char *const kScannerAppId = "fp6-wifi-qr-scan";

class WifiQr : public QObject
{
    Q_OBJECT
    Q_PROPERTY(bool scannerAvailable READ scannerAvailable CONSTANT)
    Q_PROPERTY(bool fake READ fake CONSTANT)
public:
    using QObject::QObject;

    bool fake() const { return qEnvironmentVariableIntValue("FP6_WIFIQR_FAKE") == 1; }

    bool scannerAvailable() const
    {
        if (fake())
            return true;
        return !QStandardPaths::locate(QStandardPaths::ApplicationsLocation,
                                       QString::fromLatin1(kScannerAppId) + QStringLiteral(".desktop")).isEmpty();
    }

    // Starts the scanner through the URL dispatcher (lomiri-app-launch), as
    // the network indicator does; lomiri-app-launch directly as a fallback.
    Q_INVOKABLE void launchScanner()
    {
        if (fake()) {
            Q_EMIT launched();
            return;
        }
        const QString url = QStringLiteral("application:///%1.desktop").arg(QLatin1String(kScannerAppId));
        QDBusMessage m = QDBusMessage::createMethodCall(QStringLiteral("com.lomiri.URLDispatcher"),
                                                        QStringLiteral("/com/lomiri/URLDispatcher"),
                                                        QStringLiteral("com.lomiri.URLDispatcher"),
                                                        QStringLiteral("DispatchURL"));
        m << url << QString();
        auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(m, 10000), this);
        connect(w, &QDBusPendingCallWatcher::finished, this, [this, w] {
            QDBusPendingReply<> r = *w;
            w->deleteLater();
            if (r.isError()) {
                qWarning("fp6-wifi-qr: URL dispatcher: %s; starting the scanner directly", qPrintable(r.error().message()));
                if (!QProcess::startDetached(QStringLiteral("lomiri-app-launch"), {QString::fromLatin1(kScannerAppId)})) {
                    Q_EMIT launchFailed();
                    return;
                }
            }
            Q_EMIT launched();
        });
    }

Q_SIGNALS:
    void launched();
    void launchFailed();
};

class Fp6WifiQrPlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<QrCodeItem>(uri, 1, 0, "QrCode");
        qmlRegisterType<WifiShare>(uri, 1, 0, "WifiShare");
        qmlRegisterSingletonType<WifiQr>(uri, 1, 0, "WifiQr", [](QQmlEngine *, QJSEngine *) -> QObject * {
            return new WifiQr;
        });
    }
};

#include "plugin.moc"
