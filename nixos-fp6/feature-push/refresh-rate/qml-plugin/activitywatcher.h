// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <QObject>
#include <QPointer>
#include <QTimer>

/*
 * active while the user touches or types: true on any touch, mouse button,
 * wheel, tablet or key event delivered to target (Lomiri's QQuickWindow,
 * which sees all input before qtmir forwards it to apps), false idleMs
 * after the last one. The events are only observed, never consumed.
 */
class ActivityWatcher : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QObject *target READ target WRITE setTarget NOTIFY targetChanged)
    Q_PROPERTY(int idleMs READ idleMs WRITE setIdleMs NOTIFY idleMsChanged)
    Q_PROPERTY(bool enabled READ enabled WRITE setEnabled NOTIFY enabledChanged)
    Q_PROPERTY(bool active READ active NOTIFY activeChanged)

public:
    explicit ActivityWatcher(QObject *parent = nullptr);
    ~ActivityWatcher() override;

    QObject *target() const { return m_target; }
    void setTarget(QObject *target);
    int idleMs() const { return m_idle.interval(); }
    void setIdleMs(int ms);
    bool enabled() const { return m_enabled; }
    void setEnabled(bool enabled);
    bool active() const { return m_active; }

    // Count an activity now (e.g. a key from another source).
    Q_INVOKABLE void poke();

Q_SIGNALS:
    void targetChanged();
    void idleMsChanged();
    void enabledChanged();
    void activeChanged();

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    void setActive(bool active);

    QPointer<QObject> m_target;
    QTimer m_idle;
    bool m_enabled = true;
    bool m_active = false;
};
