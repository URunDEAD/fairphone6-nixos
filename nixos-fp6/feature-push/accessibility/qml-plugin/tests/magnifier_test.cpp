// SPDX-License-Identifier: GPL-3.0-only
// Magnifier gesture tests: a QQuickView with a scene (MouseArea + Flickable)
// under the Magnifier's Scale/Translate, exactly as ShellScreen binds it, and
// real touch events through QWindowSystemInterface (QTest::touchEvent), so the
// application event filter, the replay and Qt Quick's own delivery all run.
#include <QtTest/QtTest>
#include <QtQml/QQmlEngine>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickView>

#include "colourmatrix.h"
#include "magnifier.h"

static const char *kScene = R"QML(
import QtQuick 2.15
import FP6.Accessibility 1.0
Item {
    id: root
    width: 400; height: 800
    property int clicks: 0
    property int cancels: 0
    property int longPresses: 0
    property point lastClick
    Item {
        id: scene
        width: root.width; height: root.height
        transform: [ Scale { xScale: mag.zoom; yScale: mag.zoom },
                     Translate { x: -mag.originX * mag.zoom; y: -mag.originY * mag.zoom } ]
        MouseArea {
            anchors.fill: parent
            onClicked: { root.clicks++; root.lastClick = Qt.point(mouse.x, mouse.y) }
            onCanceled: root.cancels++
            onPressAndHold: root.longPresses++
        }
        Flickable {
            objectName: "flick"
            y: 600; width: 400; height: 200
            contentHeight: 3000
            Rectangle { width: 400; height: 3000; color: "grey" }
        }
    }
    Magnifier { id: mag; objectName: "mag"; anchors.fill: parent; animationMs: 0
                slop: 10; tapDistance: 100; keyHoldMs: 300
                // generous timeouts: the test also runs on a loaded build machine
                tapTimeout: 500; multiTapTimeout: 600 }
}
)QML";

class MagnifierTest : public QObject
{
    Q_OBJECT
    QQuickView *view = nullptr;
    QTouchDevice *dev = nullptr;
    Magnifier *mag = nullptr;
    QQuickItem *root = nullptr;

    int clicks() { return root->property("clicks").toInt(); }
    QPointF lastClick() { return root->property("lastClick").toPointF(); }
    void tap(QPoint p) { QTest::touchEvent(view, dev).press(0, p); QTest::touchEvent(view, dev).release(0, p); }
    void taps(QPoint p, int n) { for (int i = 0; i < n; ++i) { tap(p); QTest::qWait(60); } }
    void settle() { QTest::qWait(900); }

private Q_SLOTS:
    void initTestCase()
    {
        qmlRegisterType<Magnifier>("FP6.Accessibility", 1, 0, "Magnifier");
        qmlRegisterType<ColourMatrix>("FP6.Accessibility", 1, 0, "ColourMatrix");
        dev = QTest::createTouchDevice();
        view = new QQuickView;
        view->setResizeMode(QQuickView::SizeViewToRootObject);
        QTemporaryFile f(QDir::tempPath() + "/magXXXXXX.qml");
        QVERIFY(f.open());
        f.write(kScene);
        f.close();
        view->setSource(QUrl::fromLocalFile(f.fileName()));
        QCOMPARE(view->status(), QQuickView::Ready);
        view->show();
        QVERIFY(QTest::qWaitForWindowExposed(view));
        root = view->rootObject();
        mag = root->findChild<Magnifier *>("mag");
        QVERIFY(mag);
    }

    void offMeansNoFilter()
    {
        QCOMPARE(mag->filtering(), false);
        tap(QPoint(50, 50));
        QCOMPARE(clicks(), 1);   // delivered at once, nothing in between
        root->setProperty("clicks", 0);
    }

    void singleTapIsDelayedThenDelivered()
    {
        mag->setTapEnabled(true);
        QCOMPARE(mag->filtering(), true);
        tap(QPoint(100, 120));
        QCOMPARE(clicks(), 0);   // held back: could be the first of three
        settle();
        QCOMPARE(clicks(), 1);
        QCOMPARE(lastClick(), QPointF(100, 120));
    }

    void doubleTapIsTwoClicks()
    {
        root->setProperty("clicks", 0);
        taps(QPoint(100, 120), 2);
        settle();
        QCOMPARE(clicks(), 2);
        QCOMPARE(mag->zoomed(), false);
    }

    void tripleTapZoomsIn()
    {
        root->setProperty("clicks", 0);
        taps(QPoint(100, 120), 3);
        settle();
        QCOMPARE(clicks(), 0);
        QVERIFY(mag->zoomed());
        QCOMPARE(mag->zoom(), 2.0);
        // the tapped point stays put: origin = p - p / 2
        QCOMPARE(mag->originX(), 50.0);
        QCOMPARE(mag->originY(), 60.0);
    }

    void zoomedTapHitsMagnifiedScenePoint()
    {
        root->setProperty("clicks", 0);
        tap(QPoint(200, 400));
        settle();
        QCOMPARE(clicks(), 1);
        QCOMPARE(lastClick(), QPointF(50 + 200 / 2.0, 60 + 400 / 2.0));
    }

    void twoFingerPan()
    {
        root->setProperty("clicks", 0);
        const qreal ox = mag->originX(), oy = mag->originY();
        QTest::touchEvent(view, dev).press(0, QPoint(100, 300)).press(1, QPoint(300, 300));
        for (int i = 1; i <= 5; ++i)
            QTest::touchEvent(view, dev).move(0, QPoint(100 - 20 * i, 300 - 10 * i)).move(1, QPoint(300 - 20 * i, 300 - 10 * i));
        QTest::touchEvent(view, dev).release(0, QPoint(0, 250)).release(1, QPoint(200, 250));
        settle();
        QCOMPARE(mag->gestureState(), QStringLiteral("idle"));
        QCOMPARE(mag->zoom(), 2.0);
        QCOMPARE(mag->originX(), ox + 100 / 2.0);  // content follows the fingers
        QCOMPARE(mag->originY(), oy + 50 / 2.0);
        QCOMPARE(clicks(), 0);
    }

    void pinchChangesLevel()
    {
        QSignalSpy committed(mag, &Magnifier::levelCommitted);
        QTest::touchEvent(view, dev).press(0, QPoint(150, 400)).press(1, QPoint(250, 400));
        for (int i = 1; i <= 5; ++i)
            QTest::touchEvent(view, dev).move(0, QPoint(150 - 10 * i, 400)).move(1, QPoint(250 + 10 * i, 400));
        QTest::touchEvent(view, dev).release(0, QPoint(100, 400)).release(1, QPoint(300, 400));
        QCOMPARE(mag->zoom(), 4.0);   // span 50 -> 100
        QCOMPARE(mag->level(), 4.0);
        QCOMPARE(committed.count(), 1);
    }

    void secondFingerCancelsAppGesture()
    {
        // A long press reaches the app (passthrough), then a second finger
        // takes over: the app gets a cancel, not a click.
        root->setProperty("clicks", 0);
        root->setProperty("cancels", 0);
        QTest::touchEvent(view, dev).press(0, QPoint(200, 200));
        QTest::qWait(700);   // longer than tapTimeout: replayed to the app
        QTest::touchEvent(view, dev).stationary(0).press(1, QPoint(300, 300));
        QTest::touchEvent(view, dev).move(0, QPoint(190, 200)).move(1, QPoint(290, 300));
        QTest::touchEvent(view, dev).release(0, QPoint(190, 200)).release(1, QPoint(290, 300));
        settle();
        QCOMPARE(clicks(), 0);
        QCOMPARE(root->property("cancels").toInt(), 1);
    }

    void tripleTapZoomsOut()
    {
        taps(QPoint(200, 200), 3);
        settle();
        QCOMPARE(mag->zoomed(), false);
        QCOMPARE(mag->zoom(), 1.0);
        QCOMPARE(mag->filtering(), true);   // still listening for the gesture
    }

    void dragPassesImmediately()
    {
        auto *flick = root->findChild<QQuickItem *>("flick");
        QVERIFY(flick);
        const qreal y0 = flick->property("contentY").toReal();
        QTest::touchEvent(view, dev).press(0, QPoint(200, 780));
        for (int i = 1; i <= 10; ++i) {
            QTest::touchEvent(view, dev).move(0, QPoint(200, 780 - 15 * i));
            QTest::qWait(10);
        }
        // scrolled while the finger is still down
        QVERIFY(flick->property("contentY").toReal() > y0 + 50);
        QTest::touchEvent(view, dev).release(0, QPoint(200, 630));
        settle();
        QCOMPARE(mag->zoomed(), false);
    }

    void longPressStillWorks()
    {
        root->setProperty("longPresses", 0);
        QTest::touchEvent(view, dev).press(0, QPoint(60, 60));
        QTest::qWait(1800);
        QTest::touchEvent(view, dev).release(0, QPoint(60, 60));
        settle();
        QCOMPARE(root->property("longPresses").toInt(), 1);
    }

    void tripleTapAndHoldIsTemporary()
    {
        mag->setLevel(2.0);
        taps(QPoint(100, 100), 2);
        QTest::touchEvent(view, dev).press(0, QPoint(100, 100));
        QTest::qWait(700);
        QVERIFY(mag->zoomed());
        QVERIFY(mag->temporary());
        QCOMPARE(mag->zoom(), 2.0);
        // finger moves: the scene point under it stays under it
        QTest::touchEvent(view, dev).move(0, QPoint(300, 600));
        QCOMPARE(mag->screenToScene(300, 600), QPointF(300, 600));
        QTest::touchEvent(view, dev).release(0, QPoint(300, 600));
        QCOMPARE(mag->zoomed(), false);
        QCOMPARE(mag->zoom(), 1.0);
    }

    void tripleTapMoveIsTemporaryToo()
    {
        taps(QPoint(100, 100), 2);
        QTest::touchEvent(view, dev).press(0, QPoint(100, 100));
        QTest::touchEvent(view, dev).move(0, QPoint(140, 100));
        QVERIFY(mag->temporary());
        QTest::touchEvent(view, dev).release(0, QPoint(140, 100));
        QCOMPARE(mag->zoomed(), false);
    }

    void holdDisabled()
    {
        mag->setHoldEnabled(false);
        root->setProperty("longPresses", 0);
        taps(QPoint(100, 100), 2);
        QTest::touchEvent(view, dev).press(0, QPoint(100, 100));
        QTest::qWait(1800);
        QTest::touchEvent(view, dev).release(0, QPoint(100, 100));
        settle();
        QCOMPARE(mag->zoomed(), false);
        QCOMPARE(root->property("longPresses").toInt(), 1);
        mag->setHoldEnabled(true);
    }

    void farApartTapsAreClicks()
    {
        root->setProperty("clicks", 0);
        tap(QPoint(50, 50));
        QTest::qWait(60);
        tap(QPoint(350, 500));
        QTest::qWait(60);
        tap(QPoint(350, 500));
        settle();
        QCOMPARE(clicks(), 3);
        QCOMPARE(mag->zoomed(), false);
    }

    void doubleTapSetting()
    {
        mag->setTapCount(2);
        taps(QPoint(100, 100), 2);
        settle();
        QVERIFY(mag->zoomed());
        taps(QPoint(100, 100), 2);
        settle();
        QVERIFY(!mag->zoomed());
        mag->setTapCount(3);
    }

    void volumeKeysShortcut()
    {
        mag->setTapEnabled(false);
        QCOMPARE(mag->filtering(), false);
        mag->setKeyShortcut(true);
        QCOMPARE(mag->filtering(), true);
        QTest::keyPress(view, Qt::Key_VolumeUp);
        QTest::keyPress(view, Qt::Key_VolumeDown);
        QTest::qWait(450);
        QVERIFY(mag->zoomed());
        QCOMPARE(mag->originX(), 100.0);   // centre: 200 - 200/2
        QTest::keyRelease(view, Qt::Key_VolumeUp);
        QTest::keyRelease(view, Qt::Key_VolumeDown);
        // a short press of both does nothing
        QTest::keyPress(view, Qt::Key_VolumeUp);
        QTest::keyPress(view, Qt::Key_VolumeDown);
        QTest::qWait(100);
        QTest::keyRelease(view, Qt::Key_VolumeDown);
        QTest::keyRelease(view, Qt::Key_VolumeUp);
        QTest::qWait(400);
        QVERIFY(mag->zoomed());
        // zoomed without tap gestures: one finger goes straight to the app
        root->setProperty("clicks", 0);
        tap(QPoint(200, 400));
        QCOMPARE(clicks(), 1);
        QCOMPARE(lastClick(), QPointF(100 + 100, 200 + 200));
        mag->toggle();
        QVERIFY(!mag->zoomed());
    }

    void controlSingletonToggles()
    {
        MagnifierControl *c = MagnifierControl::instance();
        QVERIFY(c->available());
        c->toggle(0);
        QVERIFY(mag->zoomed());
        QVERIFY(c->zoomed());
        c->toggle(100);
        QVERIFY(mag->zoomed());
        QTest::qWait(200);
        QVERIFY(!mag->zoomed());
    }

    void allOffRemovesFilter()
    {
        mag->setKeyShortcut(false);
        mag->setTapEnabled(false);
        QCOMPARE(mag->zoom(), 1.0);
        QCOMPARE(mag->filtering(), false);
    }

    void cleanupTestCase() { delete view; }
};

QTEST_MAIN(MagnifierTest)
#include "magnifier_test.moc"
