// fp6-sensor-tools TOOL: one launcher for the sensor apps; TOOL picks the
// QML app (compass, level, measure, altimeter, sound, light, gps,
// dashboard). Each app has its own desktop entry fp6-TOOL.desktop.
#include "sensorfeed.h"

#include <QDir>
#include <QGuiApplication>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickView>
#include <QStringList>
#include <QTimer>
#include <cstdio>

#ifndef FP6_QML_DIR
#define FP6_QML_DIR "qml"
#endif

int main(int argc, char *argv[])
{
    const QStringList tools{"compass", "level", "measure", "altimeter",
                            "sound", "light", "gps", "dashboard"};
    QString tool = argc > 1 ? QString::fromLocal8Bit(argv[1]) : QStringLiteral("dashboard");
    if (!tools.contains(tool)) {
        std::fprintf(stderr, "usage: fp6-sensor-tools {%s}\n", qPrintable(tools.join('|')));
        return 2;
    }
    const QString appId = QStringLiteral("fp6-") + tool;
    // GeoClue identifies Qt clients by this desktop id (QtPositioning geoclue2).
    if (qEnvironmentVariableIsEmpty("QT_GEOCLUE_APP_DESKTOP_ID"))
        qputenv("QT_GEOCLUE_APP_DESKTOP_ID", appId.toUtf8());

    QGuiApplication app(argc, argv);
    app.setOrganizationName(QStringLiteral("fp6-sensor-apps"));
    app.setApplicationName(appId);
    app.setDesktopFileName(appId);

    qmlRegisterType<SensorFeed>("Fp6.Sensors", 1, 0, "SensorFeed");

    QString qmlDir = qEnvironmentVariable("FP6_SENSOR_QML_DIR", QStringLiteral(FP6_QML_DIR));
    QQuickView view;
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    view.engine()->addImportPath(qmlDir);
    view.rootContext()->setContextProperty(QStringLiteral("appId"), appId);
    view.rootContext()->setContextProperty(QStringLiteral("appTool"), tool);
    // Preview/test hook: QML may open a given page or mode at start.
    view.rootContext()->setContextProperty(QStringLiteral("fp6Preview"), qEnvironmentVariable("FP6_PREVIEW"));
    QObject::connect(view.engine(), &QQmlEngine::quit, &app, &QGuiApplication::quit);
    view.setSource(QUrl::fromLocalFile(QDir(qmlDir).filePath(tool + QStringLiteral("/Main.qml"))));
    if (view.status() == QQuickView::Error)
        return 1;
    // Test hook for offline UI previews: FP6_GRAB=/path.png[:delay_ms]
    // [FP6_GRAB_SIZE=WxH] saves one frame and quits. Unset in the desktop
    // entries.
    const QString grab = qEnvironmentVariable("FP6_GRAB");
    const QStringList size = qEnvironmentVariable("FP6_GRAB_SIZE").split('x');
    if (size.size() == 2)
        view.resize(size[0].toInt(), size[1].toInt());
    view.show();
    if (!grab.isEmpty()) {
        view.requestActivate();  // no window manager under Xvfb
        const QStringList g = grab.split(':');
        QTimer::singleShot(g.size() > 1 ? g[1].toInt() : 1500, &view, [&view, g]() {
            view.grabWindow().save(g[0]);
            QGuiApplication::quit();
        });
    }
    return app.exec();
}
