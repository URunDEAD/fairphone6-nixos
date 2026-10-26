// SPDX-License-Identifier: GPL-3.0-only
// x86 test harness: a QQuickView on an OpenGL ES 3 context (EGL, Mesa) that
// loads capture-test.qml (FP6.ScreenRecorder + an animation), like Lomiri's
// shell window. Exit code = the QML's exit code.
#include <QGuiApplication>
#include <QQmlEngine>
#include <QQuickView>
#include <QSurfaceFormat>

int main(int argc, char **argv)
{
    QSurfaceFormat format;
    format.setRenderableType(QSurfaceFormat::OpenGLES);
    format.setVersion(3, 0);
    QSurfaceFormat::setDefaultFormat(format);
    QGuiApplication app(argc, argv);
    QQuickView view;
    QObject::connect(view.engine(), &QQmlEngine::exit, &app, &QCoreApplication::exit);
    QObject::connect(view.engine(), &QQmlEngine::quit, &app, &QCoreApplication::quit);
    view.setSource(QUrl::fromLocalFile(QString::fromLocal8Bit(argv[1])));
    if (view.status() != QQuickView::Ready)
        return 3;
    view.resize(360, 640);
    view.show();
    return app.exec();
}
