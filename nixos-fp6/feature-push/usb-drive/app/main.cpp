// fp6-usb-drive: Lomiri app (QML) over the Rust backend.
#include "usbdrive.h"

#include <QDir>
#include <QGuiApplication>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickView>
#include <QTimer>

#ifndef FP6_QML_DIR
#define FP6_QML_DIR "qml"
#endif

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);
    app.setOrganizationName(QStringLiteral("fp6-usb-drive"));
    app.setApplicationName(QStringLiteral("fp6-usb-drive"));
    app.setDesktopFileName(QStringLiteral("fp6-usb-drive"));

    qmlRegisterType<UsbDrive>("Fp6.UsbDrive", 1, 0, "UsbDrive");

    const QString qmlDir = qEnvironmentVariable("FP6_USB_DRIVE_QML_DIR", QStringLiteral(FP6_QML_DIR));
    QQuickView view;
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    view.engine()->addImportPath(qmlDir);
    // Preview/test hook: QML may open a page at start (unset in the desktop entry).
    view.rootContext()->setContextProperty(QStringLiteral("fp6Preview"), qEnvironmentVariable("FP6_PREVIEW"));
    QObject::connect(view.engine(), &QQmlEngine::quit, &app, &QGuiApplication::quit);
    view.setSource(QUrl::fromLocalFile(QDir(qmlDir).filePath(QStringLiteral("Main.qml"))));
    if (view.status() == QQuickView::Error)
        return 1;
    // FP6_GRAB=/path.png[:delay_ms] [FP6_GRAB_SIZE=WxH]: save one frame and quit.
    const QString grab = qEnvironmentVariable("FP6_GRAB");
    const QStringList size = qEnvironmentVariable("FP6_GRAB_SIZE").split('x');
    if (size.size() == 2)
        view.resize(size[0].toInt(), size[1].toInt());
    view.show();
    if (!grab.isEmpty()) {
        view.requestActivate();
        const QStringList g = grab.split(':');
        QTimer::singleShot(g.size() > 1 ? g[1].toInt() : 1500, &view, [&view, g]() {
            view.grabWindow().save(g[0]);
            QGuiApplication::quit();
        });
    }
    return app.exec();
}
