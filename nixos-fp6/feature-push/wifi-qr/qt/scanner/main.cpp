// SPDX-License-Identifier: GPL-3.0-only
// fp6-wifi-qr-scan: "Scan QR code to connect" (Settings > Wi-Fi and the
// network indicator). Back camera viewfinder through the same Qt camerabin
// -> fp6camerabinsrc -> libcamera path as Lomiri Camera and the Magnifier
// (the wrapper sets the camera stack's environment); frames go to the Rust
// QR decoder; a Wi-Fi code is joined through NetworkManager after "Join".
// Own binary and desktop entry (fp6-wifi-qr-scan.desktop, "Wi-Fi QR
// scanner"), so the camera in-use indicator names it.
#include "../common/qrcodeitem.h"
#include "qrscanfilter.h"
#include "wifijoin.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCall>
#include <QDir>
#include <QGuiApplication>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickView>
#include <QTimer>

#ifndef FP6_QML_DIR
#define FP6_QML_DIR "qml"
#endif

class ScanApp : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
    // settings:///system/wifi through the URL dispatcher
    Q_INVOKABLE void openWifiSettings()
    {
        QDBusMessage m = QDBusMessage::createMethodCall(QStringLiteral("com.lomiri.URLDispatcher"),
                                                        QStringLiteral("/com/lomiri/URLDispatcher"),
                                                        QStringLiteral("com.lomiri.URLDispatcher"),
                                                        QStringLiteral("DispatchURL"));
        m << QStringLiteral("settings:///system/wifi") << QString();
        QDBusConnection::sessionBus().asyncCall(m);
    }
};

int main(int argc, char *argv[])
{
    const QString appId = QStringLiteral("fp6-wifi-qr-scan");
    QGuiApplication app(argc, argv);
    app.setOrganizationName(appId);
    app.setApplicationName(appId);
    app.setDesktopFileName(appId);

    qmlRegisterType<QrCodeItem>("Fp6.WifiQrScan", 1, 0, "QrCode");
    qmlRegisterType<QrScanFilter>("Fp6.WifiQrScan", 1, 0, "QrScanFilter");
    qmlRegisterType<WifiJoin>("Fp6.WifiQrScan", 1, 0, "WifiJoin");

    const QString qmlDir = qEnvironmentVariable("FP6_WIFIQR_QML_DIR", QStringLiteral(FP6_QML_DIR));
    ScanApp scanApp;
    QQuickView view;
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    view.setTitle(QObject::tr("Wi-Fi QR scanner"));
    view.rootContext()->setContextProperty(QStringLiteral("appId"), appId);
    view.rootContext()->setContextProperty(QStringLiteral("scanApp"), &scanApp);
    // Preview/test hooks (unset in the desktop entry): FP6_PREVIEW=demo
    // replaces the camera by a synthetic QR code; FP6_PREVIEW_STEP picks a
    // UI state; FP6_GRAB=/path.png[:delay_ms] saves one frame and quits.
    view.rootContext()->setContextProperty(QStringLiteral("fp6Preview"), qEnvironmentVariable("FP6_PREVIEW"));
    view.rootContext()->setContextProperty(QStringLiteral("fp6PreviewStep"), qEnvironmentVariable("FP6_PREVIEW_STEP"));
    view.rootContext()->setContextProperty(QStringLiteral("fp6PreviewPayload"), qEnvironmentVariable("FP6_PREVIEW_PAYLOAD"));
    QObject::connect(view.engine(), &QQmlEngine::quit, &app, &QGuiApplication::quit);
    view.setSource(QUrl::fromLocalFile(QDir(qmlDir).filePath(QStringLiteral("Main.qml"))));
    if (view.status() == QQuickView::Error)
        return 1;
    const QString grab = qEnvironmentVariable("FP6_GRAB");
    const QStringList size = qEnvironmentVariable("FP6_GRAB_SIZE").split('x');
    if (size.size() == 2)
        view.resize(size[0].toInt(), size[1].toInt());
    view.show();
    if (!grab.isEmpty()) {
        view.requestActivate();
        const QStringList g = grab.split(':');
        QTimer::singleShot(g.size() > 1 ? g[1].toInt() : 2500, &view, [&view, g]() {
            view.grabWindow().save(g[0]);
            QGuiApplication::quit();
        });
    }
    return app.exec();
}

#include "main.moc"
