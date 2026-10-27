// SPDX-License-Identifier: GPL-3.0-only
//
// FP6.Accessibility Magnifier: Android-style screen magnification for the
// Lomiri shell. The shell (ShellScreen.qml) binds a Scale + Translate on the
// item that holds the whole scene (OrientedShell: apps, panel, launcher, OSK,
// lock screen) to zoom/originX/originY, so it is a compositor-level transform:
// Qt Quick maps every touch through it, apps receive correct coordinates.
//
// Gestures (an application-level event filter on the shell window's touch
// and key events; installed only while a trigger is on or the screen is
// zoomed, so with everything off there is no filter, no transform and no
// cost):
//  * N taps (tapCount, default 3) toggle the zoom at the tap point. As on
//    Android, taps are held back until they cannot be the start of the
//    gesture any more (multiTapTimeout), then replayed unchanged. Drags pass
//    as soon as they move beyond `slop`.
//  * N taps, keeping the last finger down (holdEnabled): magnified while held;
//    the finger pans (the scene point under the finger stays under it);
//    lifting it zooms out.
//  * Zoomed: two (or more) fingers pan and pinch (the zoom level is kept,
//    levelCommitted is emitted for the settings); one finger goes to the
//    apps. A gesture already delivered to an app is cancelled
//    (TouchCancel) when the second finger lands.
//  * keyShortcut: both volume keys held for keyHoldMs toggle the zoom at the
//    screen centre (the key events themselves are not consumed).
#pragma once

#include <QtCore/QElapsedTimer>
#include <QtCore/QPointer>
#include <QtCore/QTimer>
#include <QtCore/QVariantAnimation>
#include <QtQuick/QQuickItem>

class QTouchEvent;
class QTouchDevice;

class Magnifier : public QQuickItem
{
    Q_OBJECT
    Q_PROPERTY(bool tapEnabled READ tapEnabled WRITE setTapEnabled NOTIFY configChanged)
    Q_PROPERTY(int tapCount READ tapCount WRITE setTapCount NOTIFY configChanged)
    Q_PROPERTY(bool holdEnabled READ holdEnabled WRITE setHoldEnabled NOTIFY configChanged)
    Q_PROPERTY(bool keyShortcut READ keyShortcut WRITE setKeyShortcut NOTIFY configChanged)
    Q_PROPERTY(int keyHoldMs MEMBER m_keyHoldMs NOTIFY configChanged)
    // Preferred zoom level (pinch changes it; the shell saves it).
    Q_PROPERTY(qreal level READ level WRITE setLevel NOTIFY levelChanged)
    Q_PROPERTY(qreal minLevel MEMBER m_minLevel CONSTANT)
    Q_PROPERTY(qreal maxLevel MEMBER m_maxLevel CONSTANT)
    // Touch slop and maximum distance between the taps, in px.
    Q_PROPERTY(qreal slop MEMBER m_slop NOTIFY configChanged)
    Q_PROPERTY(qreal tapDistance MEMBER m_tapDistance NOTIFY configChanged)
    Q_PROPERTY(int tapTimeout MEMBER m_tapTimeout NOTIFY configChanged)
    Q_PROPERTY(int multiTapTimeout MEMBER m_multiTapTimeout NOTIFY configChanged)
    Q_PROPERTY(int animationMs MEMBER m_animationMs NOTIFY configChanged)
    // Current view: scale and the scene point shown at the top-left corner.
    Q_PROPERTY(qreal zoom READ zoom NOTIFY viewChanged)
    Q_PROPERTY(qreal originX READ originX NOTIFY viewChanged)
    Q_PROPERTY(qreal originY READ originY NOTIFY viewChanged)
    Q_PROPERTY(bool zoomed READ zoomed NOTIFY zoomedChanged)
    Q_PROPERTY(bool temporary READ temporary NOTIFY zoomedChanged)
    // Diagnostics / tests: the event filter is installed.
    Q_PROPERTY(bool filtering READ filtering NOTIFY filteringChanged)
    // Diagnostics / tests: "idle", "detecting", "passthrough", "panning", "holding".
    Q_PROPERTY(QString gestureState READ gestureState NOTIFY gestureStateChanged)

public:
    explicit Magnifier(QQuickItem *parent = nullptr);
    ~Magnifier() override;

    bool tapEnabled() const { return m_tapEnabled; }
    int tapCount() const { return m_tapCount; }
    bool holdEnabled() const { return m_holdEnabled; }
    bool keyShortcut() const { return m_keyShortcut; }
    qreal level() const { return m_level; }
    qreal zoom() const { return m_zoom; }
    qreal originX() const { return m_origin.x(); }
    qreal originY() const { return m_origin.y(); }
    bool zoomed() const { return m_zoomedState; }
    bool temporary() const { return m_temporary; }
    bool filtering() const { return m_filtering; }
    QString gestureState() const;

    void setTapEnabled(bool v);
    void setTapCount(int v);
    void setHoldEnabled(bool v);
    void setKeyShortcut(bool v);
    void setLevel(qreal v);

    // Zoom in at (x, y) (item coordinates; negative = centre) / out / toggle.
    Q_INVOKABLE void zoomIn(qreal x = -1, qreal y = -1);
    Q_INVOKABLE void zoomOut();
    Q_INVOKABLE void toggle(qreal x = -1, qreal y = -1);
    // Sets the view immediately (clamped), for tests.
    Q_INVOKABLE void setView(qreal zoom, qreal originX, qreal originY);
    // Maps a point on the screen (item coordinates) to the scene shown there.
    Q_INVOKABLE QPointF screenToScene(qreal x, qreal y) const;

Q_SIGNALS:
    void configChanged();
    void levelChanged();
    void viewChanged();
    void zoomedChanged();
    void filteringChanged();
    void gestureStateChanged();
    // The user changed the zoom level (pinch); save it.
    void levelCommitted(qreal level);

protected:
    bool eventFilter(QObject *obj, QEvent *ev) override;
    void itemChange(ItemChange change, const ItemChangeData &value) override;
    void geometryChanged(const QRectF &newGeometry, const QRectF &oldGeometry) override;

private:
    enum State { Idle, Detecting, PassThrough, Panning, Holding };

    bool handleTouch(QTouchEvent *te);
    void handleKey(QEvent *ev);
    void onTimer();
    void updateFilter();
    void setState(State s);
    void buffer(QTouchEvent *te);
    void replay(int count = -1);       // replay the first `count` buffered events (all: -1)
    void dropBuffer();
    void sendCancel(QTouchEvent *like);
    void startPanning(QTouchEvent *te);
    void updatePanning(QTouchEvent *te);
    void startHolding(const QPointF &pos);
    void applyView(qreal zoom, QPointF origin);
    QPointF clampOrigin(qreal zoom, QPointF origin) const;
    QPointF originFor(qreal zoom, const QPointF &screenPos, const QPointF &scenePos) const;
    void animateTo(qreal zoom, QPointF origin, bool temporary);
    void setZoomedState(bool zoomed, bool temporary);
    QPointF toItem(const QPointF &windowPos) const;

    bool m_tapEnabled = false;
    int m_tapCount = 3;
    bool m_holdEnabled = true;
    bool m_keyShortcut = false;
    int m_keyHoldMs = 2000;
    qreal m_level = 2.0;
    qreal m_minLevel = 1.25;
    qreal m_maxLevel = 8.0;
    qreal m_slop = 16;
    qreal m_tapDistance = 160;
    int m_tapTimeout = 300;
    int m_multiTapTimeout = 300;
    int m_animationMs = 200;

    qreal m_zoom = 1;
    QPointF m_origin;
    bool m_zoomedState = false;
    bool m_temporary = false;
    bool m_filtering = false;
    QPointer<QWindow> m_window;

    State m_state = Idle;
    QList<QTouchEvent *> m_buffer;
    int m_completeTaps = 0;       // buffered events belonging to completed taps
    int m_taps = 0;
    QPointF m_firstTapPos, m_downPos;
    bool m_fingerDown = false;
    bool m_moved = false;
    QElapsedTimer m_downClock;
    QTimer m_timer;
    bool m_replaying = false;

    // panning baseline
    int m_panCount = 0;
    QPointF m_panCentroid;
    qreal m_panSpan = 0;
    qreal m_panZoom = 1;
    QPointF m_panOrigin;
    bool m_panZoomChanged = false;

    // keys
    bool m_volUp = false, m_volDown = false;
    QTimer m_keyTimer;

    // animation
    QVariantAnimation m_anim;
    qreal m_animFromZoom = 1, m_animToZoom = 1;
    QPointF m_animFromOrigin, m_animToOrigin;
};

// Singleton (FP6.Accessibility MagnifierControl): lets other shell parts
// (the quick-settings tile) toggle the active screen's magnifier.
class MagnifierControl : public QObject
{
    Q_OBJECT
    Q_PROPERTY(bool available READ available NOTIFY changed)
    Q_PROPERTY(bool zoomed READ zoomed NOTIFY changed)
public:
    static MagnifierControl *instance();
    bool available() const { return !m_magnifier.isNull(); }
    bool zoomed() const { return m_magnifier && m_magnifier->zoomed(); }
    // Toggles after delayMs (e.g. after the panel has closed).
    Q_INVOKABLE void toggle(int delayMs = 0);
    void attach(Magnifier *m);
    void detach(Magnifier *m);
Q_SIGNALS:
    void changed();
private:
    QPointer<Magnifier> m_magnifier;
};
