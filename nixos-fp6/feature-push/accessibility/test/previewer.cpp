// Offscreen-ish preview grabber: loads a QML scene in a QQuickView (a real
// GL window under xvfb + Mesa, so FP6.DisplayComfort ColorFilter renders),
// waits, reads the window back (grabWindow includes the colour filter pass)
// and saves a PNG.   fp6-a11y-previewer SCENE.qml OUT.png [WAIT_MS]
#include <QtCore/QTimer>
#include <QtGui/QGuiApplication>
#include <QtGui/QImage>
#include <QtQuick/QQuickView>
#include <cstdio>

int main(int argc, char **argv)
{
    QGuiApplication app(argc, argv);
    if (argc < 3) { std::fprintf(stderr, "usage: %s SCENE.qml OUT.png [WAIT_MS]\n", argv[0]); return 2; }
    QQuickView view;
    view.setResizeMode(QQuickView::SizeViewToRootObject);
    view.setColor(Qt::black);
    view.setSource(QUrl::fromLocalFile(QString::fromLocal8Bit(argv[1])));
    if (view.status() != QQuickView::Ready) return 3;
    view.show();
    const int wait = argc > 3 ? atoi(argv[3]) : 2500;
    const QString out = QString::fromLocal8Bit(argv[2]);
    QTimer::singleShot(wait, [&] {
        const QImage img = view.grabWindow();
        const bool ok = img.save(out);
        std::printf("%s %s %dx%d\n", ok ? "saved" : "FAILED", qPrintable(out), img.width(), img.height());
        app.exit(ok ? 0 : 1);
    });
    return app.exec();
}
