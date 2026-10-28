// SPDX-License-Identifier: GPL-3.0-only
#include "activitywatcher.h"

#include <QEvent>
#include <QMouseEvent>

ActivityWatcher::ActivityWatcher(QObject *parent)
    : QObject(parent)
{
    m_idle.setSingleShot(true);
    m_idle.setInterval(3000);
    connect(&m_idle, &QTimer::timeout, this, [this] { setActive(false); });
}

ActivityWatcher::~ActivityWatcher()
{
    if (m_target)
        m_target->removeEventFilter(this);
}

void ActivityWatcher::setTarget(QObject *target)
{
    if (target == m_target)
        return;
    if (m_target)
        m_target->removeEventFilter(this);
    m_target = target;
    if (m_target)
        m_target->installEventFilter(this);
    Q_EMIT targetChanged();
}

void ActivityWatcher::setIdleMs(int ms)
{
    ms = qBound(100, ms, 60000);
    if (ms == m_idle.interval())
        return;
    m_idle.setInterval(ms);
    Q_EMIT idleMsChanged();
}

void ActivityWatcher::setEnabled(bool enabled)
{
    if (enabled == m_enabled)
        return;
    m_enabled = enabled;
    if (!m_enabled) {
        m_idle.stop();
        setActive(false);
    }
    Q_EMIT enabledChanged();
}

void ActivityWatcher::poke()
{
    if (!m_enabled)
        return;
    setActive(true);
    m_idle.start();
}

bool ActivityWatcher::eventFilter(QObject *watched, QEvent *event)
{
    if (m_enabled) {
        switch (event->type()) {
        case QEvent::TouchBegin:
        case QEvent::TouchUpdate:
        case QEvent::MouseButtonPress:
        case QEvent::Wheel:
        case QEvent::TabletPress:
        case QEvent::TabletMove:
        case QEvent::KeyPress:
            poke();
            break;
        case QEvent::MouseMove:
            if (static_cast<QMouseEvent *>(event)->buttons() != Qt::NoButton)
                poke();
            break;
        case QEvent::TouchEnd:
        case QEvent::MouseButtonRelease:
            // kinetic scrolling goes on after the finger lifts
            if (m_active)
                m_idle.start();
            break;
        default:
            break;
        }
    }
    return QObject::eventFilter(watched, event);
}

void ActivityWatcher::setActive(bool active)
{
    if (active == m_active)
        return;
    m_active = active;
    Q_EMIT activeChanged();
}
