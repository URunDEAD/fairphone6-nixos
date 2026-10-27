// SPDX-License-Identifier: GPL-3.0-only
#include "magnifier.h"

#include <QtCore/QCoreApplication>
#include <QtGui/QKeyEvent>
#include <QtGui/QTouchEvent>
#include <QtGui/QWindow>
#include <QtQuick/QQuickWindow>

#include <cmath>

namespace {

QList<QTouchEvent::TouchPoint> activePoints(const QTouchEvent *te)
{
    QList<QTouchEvent::TouchPoint> out;
    for (const auto &tp : te->touchPoints())
        if (tp.state() != Qt::TouchPointReleased)
            out << tp;
    return out;
}

qreal dist(const QPointF &a, const QPointF &b)
{
    return std::hypot(a.x() - b.x(), a.y() - b.y());
}

QTouchEvent *copyTouch(const QTouchEvent *te)
{
    auto *c = new QTouchEvent(te->type(), te->device(), te->modifiers(), te->touchPointStates(),
                              te->touchPoints());
    c->setTimestamp(te->timestamp());
    c->setWindow(te->window());
    c->setTarget(te->target());
    return c;
}

} // namespace

Magnifier::Magnifier(QQuickItem *parent)
    : QQuickItem(parent)
{
    // No contents, no input of its own: the item only tracks the window.
    setFlag(ItemHasContents, false);
    setAcceptedMouseButtons(Qt::NoButton);
    m_timer.setSingleShot(true);
    connect(&m_timer, &QTimer::timeout, this, &Magnifier::onTimer);
    m_keyTimer.setSingleShot(true);
    connect(&m_keyTimer, &QTimer::timeout, this, [this] {
        if (m_keyShortcut && m_volUp && m_volDown)
            toggle();
    });
    m_anim.setStartValue(0.0);
    m_anim.setEndValue(1.0);
    m_anim.setEasingCurve(QEasingCurve::OutCubic);
    connect(&m_anim, &QVariantAnimation::valueChanged, this, [this](const QVariant &v) {
        const qreal t = v.toReal();
        const qreal z = m_animFromZoom + (m_animToZoom - m_animFromZoom) * t;
        const QPointF o = m_animFromOrigin + (m_animToOrigin - m_animFromOrigin) * t;
        applyView(z, o);
    });
    connect(&m_anim, &QVariantAnimation::finished, this, [this] {
        applyView(m_animToZoom, m_animToOrigin);
        if (m_animToZoom <= 1.0001)
            setZoomedState(false, false);
        updateFilter();
    });
    MagnifierControl::instance()->attach(this);
}

Magnifier::~Magnifier()
{
    if (m_filtering)
        QCoreApplication::instance()->removeEventFilter(this);
    dropBuffer();
    MagnifierControl::instance()->detach(this);
}

QString Magnifier::gestureState() const
{
    switch (m_state) {
    case Detecting: return QStringLiteral("detecting");
    case PassThrough: return QStringLiteral("passthrough");
    case Panning: return QStringLiteral("panning");
    case Holding: return QStringLiteral("holding");
    default: return QStringLiteral("idle");
    }
}

void Magnifier::setTapEnabled(bool v)
{
    if (v == m_tapEnabled) return;
    m_tapEnabled = v;
    Q_EMIT configChanged();
    updateFilter();
}

void Magnifier::setTapCount(int v)
{
    v = qBound(2, v, 4);
    if (v == m_tapCount) return;
    m_tapCount = v;
    Q_EMIT configChanged();
}

void Magnifier::setHoldEnabled(bool v)
{
    if (v == m_holdEnabled) return;
    m_holdEnabled = v;
    Q_EMIT configChanged();
}

void Magnifier::setKeyShortcut(bool v)
{
    if (v == m_keyShortcut) return;
    m_keyShortcut = v;
    if (!v) m_keyTimer.stop();
    Q_EMIT configChanged();
    updateFilter();
}

void Magnifier::setLevel(qreal v)
{
    v = qBound(m_minLevel, v, m_maxLevel);
    if (qFuzzyCompare(v, m_level)) return;
    m_level = v;
    Q_EMIT levelChanged();
}

void Magnifier::itemChange(ItemChange change, const ItemChangeData &value)
{
    QQuickItem::itemChange(change, value);
    if (change == ItemSceneChange) {
        m_window = value.window;
        updateFilter();
    }
}

void Magnifier::geometryChanged(const QRectF &newGeometry, const QRectF &oldGeometry)
{
    QQuickItem::geometryChanged(newGeometry, oldGeometry);
    if (m_zoom > 1)
        applyView(m_zoom, m_origin);
}

void Magnifier::updateFilter()
{
    // Application-level filter: runs before every object filter of the window
    // (Lomiri's TouchRegistry, WindowInputMonitor, ...), so the gestures see
    // the touches first. Only while something can use it.
    const bool want = m_window && (m_tapEnabled || m_keyShortcut || m_zoomedState || m_zoom > 1.0001
                                   || m_state != Idle);
    if (want == m_filtering) return;
    m_filtering = want;
    if (want) {
        QCoreApplication::instance()->installEventFilter(this);
    } else {
        QCoreApplication::instance()->removeEventFilter(this);
        m_volUp = m_volDown = false;
        m_keyTimer.stop();
    }
    Q_EMIT filteringChanged();
}

void Magnifier::setState(State s)
{
    if (s == m_state) return;
    m_state = s;
    Q_EMIT gestureStateChanged();
    if (s == Idle) updateFilter();
}

QPointF Magnifier::toItem(const QPointF &windowPos) const
{
    return mapFromScene(windowPos);
}

bool Magnifier::eventFilter(QObject *obj, QEvent *ev)
{
    if (obj != m_window.data() || m_replaying)
        return false;
    switch (ev->type()) {
    case QEvent::TouchBegin:
    case QEvent::TouchUpdate:
    case QEvent::TouchEnd:
        return handleTouch(static_cast<QTouchEvent *>(ev));
    case QEvent::TouchCancel:
        m_timer.stop();
        dropBuffer();
        if (m_state == Holding)
            zoomOut();
        setState(Idle);
        return false;
    case QEvent::KeyPress:
    case QEvent::KeyRelease:
        handleKey(ev);
        return false;
    default:
        return false;
    }
}

void Magnifier::handleKey(QEvent *ev)
{
    if (!m_keyShortcut) return;
    auto *ke = static_cast<QKeyEvent *>(ev);
    if (ke->isAutoRepeat()) return;
    const bool down = ev->type() == QEvent::KeyPress;
    if (ke->key() == Qt::Key_VolumeUp) m_volUp = down;
    else if (ke->key() == Qt::Key_VolumeDown) m_volDown = down;
    else return;
    if (m_volUp && m_volDown) {
        if (!m_keyTimer.isActive()) m_keyTimer.start(m_keyHoldMs);
    } else {
        m_keyTimer.stop();
    }
}

void Magnifier::buffer(QTouchEvent *te)
{
    m_buffer << copyTouch(te);
}

void Magnifier::replay(int count)
{
    if (count < 0 || count > m_buffer.size()) count = m_buffer.size();
    const QList<QTouchEvent *> events = m_buffer.mid(0, count);
    m_buffer = m_buffer.mid(count);
    m_completeTaps = 0;
    for (QTouchEvent *e : events) {
        if (m_window) {
            m_replaying = true;
            QCoreApplication::sendEvent(m_window.data(), e);
            m_replaying = false;
        }
        delete e;
    }
}

void Magnifier::dropBuffer()
{
    qDeleteAll(m_buffer);
    m_buffer.clear();
    m_completeTaps = 0;
}

void Magnifier::sendCancel(QTouchEvent *like)
{
    if (!m_window) return;
    QTouchEvent cancel(QEvent::TouchCancel, like->device(), like->modifiers(), Qt::TouchPointStates(),
                       QList<QTouchEvent::TouchPoint>());
    cancel.setWindow(m_window.data());
    cancel.setTimestamp(like->timestamp());
    m_replaying = true;
    QCoreApplication::sendEvent(m_window.data(), &cancel);
    m_replaying = false;
}

bool Magnifier::handleTouch(QTouchEvent *te)
{
    const auto active = activePoints(te);
    const auto &all = te->touchPoints();
    const QPointF pos = all.isEmpty() ? QPointF() : toItem(all.first().pos());
    const bool isZoomed = m_zoomedState && !m_temporary;

    switch (m_state) {
    case Idle:
        if (te->type() != QEvent::TouchBegin)
            return false;
        if (isZoomed && active.size() >= 2) {
            startPanning(te);
            return true;
        }
        if (m_tapEnabled && all.size() == 1) {
            m_taps = 0;
            m_firstTapPos = m_downPos = pos;
            m_fingerDown = true;
            m_moved = false;
            m_downClock.start();
            buffer(te);
            setState(Detecting);
            updateFilter();
            m_timer.start(m_tapTimeout);
            return true;
        }
        if (isZoomed)
            setState(PassThrough);
        return false;

    case Detecting:
        if (te->type() == QEvent::TouchBegin) {
            m_timer.stop();
            if (all.size() >= 2 && isZoomed) {
                replay();
                startPanning(te);
                return true;
            }
            if (all.size() >= 2 || dist(pos, m_firstTapPos) > m_tapDistance) {
                replay();
                setState(PassThrough);
                return false;
            }
            buffer(te);
            m_downPos = pos;
            m_fingerDown = true;
            m_moved = false;
            m_downClock.start();
            m_timer.start(m_tapTimeout);
            return true;
        }
        if (te->type() == QEvent::TouchUpdate) {
            if (active.size() >= 2) {
                m_timer.stop();
                if (isZoomed) {
                    // The current finger's events were never delivered.
                    replay(m_completeTaps);
                    dropBuffer();
                    startPanning(te);
                    return true;
                }
                replay();
                setState(PassThrough);
                return false;
            }
            if (!active.isEmpty() && dist(toItem(active.first().pos()), m_downPos) > m_slop) {
                m_timer.stop();
                if (m_holdEnabled && m_taps == m_tapCount - 1) {
                    dropBuffer();
                    startHolding(toItem(active.first().pos()));
                    return true;
                }
                replay();
                setState(PassThrough);
                return false;
            }
            buffer(te);
            return true;
        }
        if (te->type() == QEvent::TouchEnd) {
            m_timer.stop();
            m_fingerDown = false;
            if (m_downClock.elapsed() <= m_tapTimeout) {
                ++m_taps;
                buffer(te);
                m_completeTaps = m_buffer.size();
                if (m_taps >= m_tapCount) {
                    dropBuffer();
                    setState(Idle);
                    toggle(m_firstTapPos.x(), m_firstTapPos.y());
                    return true;
                }
                m_timer.start(m_multiTapTimeout);
                return true;
            }
            replay();
            setState(Idle);
            return false;
        }
        return false;

    case PassThrough:
        if (isZoomed && active.size() >= 2) {
            sendCancel(te);
            startPanning(te);
            return true;
        }
        if (te->type() == QEvent::TouchEnd)
            setState(Idle);
        return false;

    case Panning:
        if (te->type() == QEvent::TouchEnd) {
            if (m_panZoomChanged)
                Q_EMIT levelCommitted(m_level);
            setState(Idle);
            return true;
        }
        updatePanning(te);
        return true;

    case Holding:
        if (te->type() == QEvent::TouchEnd) {
            setState(Idle);
            zoomOut();
            return true;
        }
        if (!active.isEmpty()) {
            // The scene point under the finger stays under it (so every
            // edge can be reached).
            const QPointF p = toItem(active.first().pos());
            if (m_anim.state() == QAbstractAnimation::Running)
                m_animToOrigin = originFor(m_animToZoom, p, p);
            else
                applyView(m_zoom, originFor(m_zoom, p, p));
        }
        return true;
    }
    return false;
}

void Magnifier::onTimer()
{
    if (m_state != Detecting)
        return;
    if (m_fingerDown) {
        // Held: the magnify-while-held gesture, or a long press for the app.
        if (m_holdEnabled && m_taps == m_tapCount - 1) {
            dropBuffer();
            startHolding(m_downPos);
            return;
        }
        replay();
        setState(PassThrough);
        return;
    }
    // No further tap in time: these were ordinary taps.
    replay();
    setState(Idle);
}

void Magnifier::startPanning(QTouchEvent *te)
{
    m_timer.stop();
    m_anim.stop();
    setState(Panning);
    m_panCount = -1;   // baseline on the next update (below)
    m_panZoomChanged = false;
    updatePanning(te);
}

void Magnifier::updatePanning(QTouchEvent *te)
{
    const auto active = activePoints(te);
    if (active.isEmpty()) return;
    QPointF c;
    for (const auto &tp : active) c += toItem(tp.pos());
    c /= active.size();
    qreal span = 0;
    for (const auto &tp : active) span += dist(toItem(tp.pos()), c);
    span /= active.size();
    if (active.size() != m_panCount) {
        m_panCount = active.size();
        m_panCentroid = c;
        m_panSpan = span;
        m_panZoom = m_zoom;
        m_panOrigin = m_origin;
        return;
    }
    qreal z = m_panZoom;
    if (m_panCount >= 2 && m_panSpan > 1 && span > 1)
        z = qBound(m_minLevel, m_panZoom * span / m_panSpan, m_maxLevel);
    // The scene point under the starting centroid follows the centroid.
    const QPointF q = m_panOrigin + m_panCentroid / m_panZoom;
    applyView(z, q - c / z);
    if (!qFuzzyCompare(z, m_panZoom)) {
        m_panZoomChanged = true;
        setLevel(z);
    }
}

void Magnifier::startHolding(const QPointF &pos)
{
    setState(Holding);
    setZoomedState(true, true);
    animateTo(m_level, originFor(m_level, pos, pos), true);
}

QPointF Magnifier::clampOrigin(qreal z, QPointF o) const
{
    const qreal w = width(), h = height();
    const qreal maxX = qMax<qreal>(0, w - w / z), maxY = qMax<qreal>(0, h - h / z);
    return QPointF(qBound<qreal>(0, o.x(), maxX), qBound<qreal>(0, o.y(), maxY));
}

QPointF Magnifier::originFor(qreal z, const QPointF &screenPos, const QPointF &scenePos) const
{
    // origin + screenPos / z = scenePos
    return clampOrigin(z, scenePos - screenPos / z);
}

QPointF Magnifier::screenToScene(qreal x, qreal y) const
{
    return m_origin + QPointF(x, y) / m_zoom;
}

void Magnifier::applyView(qreal z, QPointF o)
{
    z = qMax<qreal>(1, z);
    o = z > 1.0001 ? clampOrigin(z, o) : QPointF();
    if (qFuzzyCompare(z, m_zoom) && o == m_origin) return;
    m_zoom = z;
    m_origin = o;
    Q_EMIT viewChanged();
}

void Magnifier::setView(qreal z, qreal ox, qreal oy)
{
    m_anim.stop();
    z = qBound<qreal>(1, z, m_maxLevel);
    applyView(z, QPointF(ox, oy));
    setZoomedState(z > 1.0001, false);
    updateFilter();
}

void Magnifier::animateTo(qreal z, QPointF o, bool temporary)
{
    m_anim.stop();
    m_animFromZoom = m_zoom;
    m_animFromOrigin = m_origin;
    m_animToZoom = z;
    m_animToOrigin = z > 1.0001 ? clampOrigin(z, o) : QPointF();
    if (z > 1.0001)
        setZoomedState(true, temporary);
    updateFilter();
    if (m_animationMs <= 0) {
        applyView(m_animToZoom, m_animToOrigin);
        if (z <= 1.0001) setZoomedState(false, false);
        updateFilter();
        return;
    }
    m_anim.setDuration(m_animationMs);
    m_anim.start();
}

void Magnifier::setZoomedState(bool zoomed, bool temporary)
{
    if (zoomed == m_zoomedState && temporary == m_temporary) return;
    m_zoomedState = zoomed;
    m_temporary = zoomed && temporary;
    Q_EMIT zoomedChanged();
    Q_EMIT MagnifierControl::instance()->changed();
}

void Magnifier::zoomIn(qreal x, qreal y)
{
    if (x < 0 || y < 0) { x = width() / 2; y = height() / 2; }
    const QPointF p(x, y);
    // The tapped scene point stays where it was tapped.
    animateTo(m_level, originFor(m_level, p, p), false);
}

void Magnifier::zoomOut()
{
    m_timer.stop();
    animateTo(1, QPointF(), false);
}

void Magnifier::toggle(qreal x, qreal y)
{
    if (m_zoomedState && !m_temporary) zoomOut();
    else zoomIn(x, y);
}

// ---- MagnifierControl

MagnifierControl *MagnifierControl::instance()
{
    static MagnifierControl *s = new MagnifierControl;
    return s;
}

void MagnifierControl::toggle(int delayMs)
{
    if (!m_magnifier) return;
    QPointer<Magnifier> m = m_magnifier;
    if (delayMs <= 0) { m->toggle(); return; }
    QTimer::singleShot(delayMs, this, [m] { if (m) m->toggle(); });
}

void MagnifierControl::attach(Magnifier *m)
{
    if (m_magnifier) return;   // first screen wins
    m_magnifier = m;
    Q_EMIT changed();
}

void MagnifierControl::detach(Magnifier *m)
{
    if (m_magnifier != m) return;
    m_magnifier = nullptr;
    Q_EMIT changed();
}
