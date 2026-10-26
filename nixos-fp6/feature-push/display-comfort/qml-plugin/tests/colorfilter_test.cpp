// SPDX-License-Identifier: GPL-3.0-only
// Pixel test for ColorFilter: renders colour patches under the filter in a
// real QQuickWindow (xvfb + Mesa on the laptop, or the phone), grabs the
// window and compares with the expected matrix result.
//   colorfilter-test [--expect-path blend|fetch|copy]
#include <QtGui/QGuiApplication>
#include <QtGui/QImage>
#include <QtGui/QVector3D>
#include <QtQml/QQmlEngine>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickView>
#include <QtCore/QElapsedTimer>
#include <QtCore/QTimer>
#include <QtCore/QTemporaryDir>
#include <QtCore/QFile>
#include <QtCore/QEventLoop>

#include <cmath>
#include <cstdio>

#include "colorfilter.h"

static int failures = 0;

static void check(bool ok, const char *what)
{
    std::printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok) ++failures;
}

static QColor expected(QColor in, const float rgb[3], float grey)
{
    float m[9];
    ColorFilter::matrix(rgb, grey, m);
    const float v[3] = { float(in.redF()), float(in.greenF()), float(in.blueF()) };
    float o[3];
    for (int r = 0; r < 3; ++r)
        o[r] = qBound(0.f, m[r * 3] * v[0] + m[r * 3 + 1] * v[1] + m[r * 3 + 2] * v[2], 1.f);
    return QColor::fromRgbF(o[0], o[1], o[2]);
}

static bool near(QColor a, QColor b, int tol = 3)
{
    return std::abs(a.red() - b.red()) <= tol && std::abs(a.green() - b.green()) <= tol
        && std::abs(a.blue() - b.blue()) <= tol;
}

int main(int argc, char **argv)
{
    QGuiApplication app(argc, argv);
    QString expectPath;
    for (int i = 1; i + 1 < argc; ++i)
        if (QByteArray(argv[i]) == "--expect-path") expectPath = QString::fromLatin1(argv[i + 1]);

    qmlRegisterType<ColorFilter>("FP6.DisplayComfort", 1, 0, "ColorFilter");

    // Temperature helper sanity.
    const QVector3D n = ColorFilter::temperatureToRgb(6500);
    check(std::fabs(n.x() - 1) < 1e-3 && std::fabs(n.y() - 1) < 1e-3 && std::fabs(n.z() - 1) < 1e-3,
          "6500 K is neutral");
    const QVector3D w = ColorFilter::temperatureToRgb(3400);
    check(w.x() > 0.99f && w.y() < 0.85f && w.z() < 0.65f && w.y() > w.z(), "3400 K is warm (r > g > b)");
    std::printf("INFO 3400K rgb %.3f %.3f %.3f; 2700K %.3f %.3f %.3f\n", w.x(), w.y(), w.z(),
                ColorFilter::temperatureToRgb(2700).x(), ColorFilter::temperatureToRgb(2700).y(),
                ColorFilter::temperatureToRgb(2700).z());

    QQuickView view;
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    view.setColor(Qt::black);
    QTemporaryDir dir;
    const QString qmlPath = dir.filePath(QStringLiteral("test.qml"));
    {
        QFile f(qmlPath);
        f.open(QIODevice::WriteOnly);
        f.write("import QtQuick 2.15\nimport FP6.DisplayComfort 1.0\n"
                "Item { width: 400; height: 100\n"
                "  Row { Repeater { model: ['#ffffff', '#ff0000', '#00ff00', '#0000ff', '#808080', '#c08040', '#203040', '#000000']\n"
                "    Rectangle { width: 50; height: 100; color: modelData } } }\n"
                "  ColorFilter { objectName: 'filter'; anchors.fill: parent; visible: active } }\n");
    }
    view.setSource(QUrl::fromLocalFile(qmlPath));
    if (view.status() != QQuickView::Ready) {
        std::printf("FAIL qml load\n");
        return 2;
    }
    view.resize(400, 100);
    view.show();

    auto *filter = view.rootObject()->findChild<ColorFilter *>(QStringLiteral("filter"));
    const QColor patches[8] = { "#ffffff", "#ff0000", "#00ff00", "#0000ff",
                                "#808080", "#c08040", "#203040", "#000000" };

    auto grabAndCheck = [&](const float rgb[3], float grey, const char *name) {
        QElapsedTimer t;
        t.start();
        // Let a couple of frames run so the path is reported back.
        for (int i = 0; i < 6; ++i) {
            filter->update();
            QEventLoop l;
            QTimer::singleShot(30, &l, &QEventLoop::quit);
            l.exec();
        }
        const QImage img = view.grabWindow();
        bool ok = true;
        for (int i = 0; i < 8; ++i) {
            const QColor got = img.pixelColor(i * 50 + 25, 50);
            const QColor want = expected(patches[i], rgb, grey);
            if (!near(got, want)) {
                ok = false;
                std::printf("  patch %d: got %s want %s\n", i, qPrintable(got.name()), qPrintable(want.name()));
            }
        }
        char what[160];
        std::snprintf(what, sizeof what, "%s (path '%s')", name, qPrintable(filter->path()));
        check(ok, what);
    };

    const float id[3] = { 1, 1, 1 };
    grabAndCheck(id, 0, "identity: no node");
    check(!filter->active() && filter->path().isEmpty(), "identity is inactive and not drawn");

    filter->setTemperature(3400);
    const QVector3D c = ColorFilter::temperatureToRgb(3400);
    const float warm[3] = { c.x(), c.y(), c.z() };
    grabAndCheck(warm, 0, "night light 3400 K");
    check(filter->path() == QLatin1String("blend"), "night light alone uses the multiply blend");

    filter->setGreyscale(1);
    grabAndCheck(warm, 1, "night light + greyscale");
    if (!expectPath.isEmpty())
        check(filter->path() == expectPath, "greyscale uses the expected path");

    filter->setTemperature(6500);
    grabAndCheck(id, 1, "greyscale only");
    filter->setGreyscale(0.5);
    grabAndCheck(id, 0.5f, "half greyscale");
    filter->setGreyscale(0);
    grabAndCheck(id, 0, "back to identity");
    check(!filter->active(), "inactive again");

    std::printf("%s (%d failures)\n", failures ? "FAILED" : "ALL PASS", failures);
    return failures ? 1 : 0;
}
