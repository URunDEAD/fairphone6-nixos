// SPDX-License-Identifier: GPL-3.0-only
// Offscreen test of FP6.RefreshRate: PanelRate against a fake sysfs
// directory, ActivityWatcher against a QWindow. Exit code 0 = pass.
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QGuiApplication>
#include <QMouseEvent>
#include <QTemporaryDir>
#include <QTouchDevice>
#include <QWindow>
#include <cstdio>
#include <unistd.h>

#include "activitywatcher.h"
#include "panelrate.h"

static int failures = 0;
#define CHECK(cond) do { if (!(cond)) { std::fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond); failures++; } } while (0)

static void spin(int ms)
{
    QElapsedTimer t;
    t.start();
    while (t.elapsed() < ms)
        QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
}

static void writeFile(const QString &f, const QByteArray &data)
{
    QFile file(f);
    file.open(QIODevice::WriteOnly | QIODevice::Truncate);
    file.write(data);
}

static QByteArray readFile(const QString &f)
{
    QFile file(f);
    file.open(QIODevice::ReadOnly);
    return file.readAll();
}

int main(int argc, char **argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication app(argc, argv);

    // PanelRate against a fake attribute
    QTemporaryDir dir;
    const QString attr = dir.filePath(QStringLiteral("panel_rate_hz"));
    writeFile(attr, "120\n");
    writeFile(dir.filePath(QStringLiteral("panel_rates")), "120 60 30 10\n");
    {
        PanelRate pr;
        pr.setPath(attr);
        CHECK(pr.available());
        CHECK(pr.current() == 120);
        CHECK(pr.rates() == QStringLiteral("120 60 30 10"));
        CHECK(pr.setRate(60));
        CHECK(pr.waitIdle());
        spin(50);
        CHECK(readFile(attr) == "60\n");
        CHECK(pr.current() == 60);
        CHECK(pr.lastError().isEmpty());
        // coalescing: only the latest value matters
        pr.setRate(30);
        pr.setRate(10);
        pr.setRate(120);
        CHECK(pr.waitIdle());
        spin(50);
        CHECK(readFile(attr) == "120\n");
        CHECK(pr.current() == 120);
        CHECK(!pr.setRate(0));
    }
    {
        // read-only attribute (no udev rule): not available, no writes
        QFile::setPermissions(attr, QFileDevice::ReadOwner);
        PanelRate pr;
        pr.setPath(attr);
        CHECK(!pr.available() || ::getuid() == 0);
        if (!pr.available())
            CHECK(!pr.setRate(60));
        QFile::setPermissions(attr, QFileDevice::ReadOwner | QFileDevice::WriteOwner);
    }
    {
        PanelRate pr;
        pr.setPath(dir.filePath(QStringLiteral("missing")));
        CHECK(!pr.available());
        CHECK(pr.current() == 0);
    }

    // ActivityWatcher
    QWindow window;
    window.resize(100, 100);
    ActivityWatcher w;
    w.setIdleMs(200);
    w.setTarget(&window);
    CHECK(!w.active());
    QMouseEvent press(QEvent::MouseButtonPress, QPointF(10, 10), Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &press);
    CHECK(w.active());
    spin(100);
    CHECK(w.active());
    QMouseEvent move(QEvent::MouseMove, QPointF(20, 20), Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &move);
    spin(150);
    CHECK(w.active()); // the move with a button held restarted the timer
    spin(200);
    CHECK(!w.active());
    // a hover move (no button) is not activity
    QMouseEvent hover(QEvent::MouseMove, QPointF(30, 30), Qt::NoButton, Qt::NoButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &hover);
    CHECK(!w.active());
    // touch
    QTouchDevice *dev = new QTouchDevice;
    dev->setType(QTouchDevice::TouchScreen);
    QTouchEvent touch(QEvent::TouchBegin, dev, Qt::NoModifier, Qt::TouchPointPressed, {});
    QCoreApplication::sendEvent(&window, &touch);
    CHECK(w.active());
    w.setEnabled(false);
    CHECK(!w.active());
    QCoreApplication::sendEvent(&window, &press);
    CHECK(!w.active());

    if (failures == 0)
        std::printf("refreshrate-test: all checks passed\n");
    return failures ? 1 : 0;
}
