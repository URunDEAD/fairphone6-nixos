// SPDX-License-Identifier: LGPL-3.0-only
// Fingerprint ID: hosts lomiri-settings-components' own Fingerprint pages
// (the ones System Settings > Security & Privacy shows) in a standalone window.
#include <QGuiApplication>
#include <QQuickView>
#include <QUrl>
#include <QImage>
#include <QTimer>

int main(int argc, char** argv)
{
    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("fp6-fingerprint-id"));
    QQuickView view;
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    view.setTitle(QStringLiteral("Fingerprint ID"));
    view.setSource(QUrl::fromLocalFile(QStringLiteral(FP6_QML_DIR "/Main.qml")));
    if (view.status() == QQuickView::Error) return 1;
    view.show();
    // Headless self-check (QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software):
    // FP6_FPID_SNAPSHOT=<png> saves the window after 4 s and quits.
    const QByteArray snap = qgetenv("FP6_FPID_SNAPSHOT");
    if (!snap.isEmpty())
    {
        view.resize(540, 1170);
        QTimer::singleShot(4000, [&view, snap]() {
            view.grabWindow().save(QString::fromLocal8Bit(snap));
            QGuiApplication::quit();
        });
    }
    return app.exec();
}
