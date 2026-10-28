// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <QMutex>
#include <QObject>
#include <QString>
#include <QThread>
#include <QWaitCondition>

class PanelRateWriter;

/*
 * The NT37705 panel's runtime rate (kernel candidate 0010):
 *   /sys/bus/mipi-dsi/devices/<dsi>/panel_rate_hz  (rw, 0 while off)
 *   /sys/bus/mipi-dsi/devices/<dsi>/panel_rates    (ro, accepted values)
 * available is false on kernels without it (kernel6), and every call is
 * then a no-op. Writes go to a worker thread (a DSI command transfer can
 * wait for the current frame), coalesced to the latest value, so the
 * shell's GUI thread never blocks on the panel.
 */
class PanelRate : public QObject
{
    Q_OBJECT
    // the attribute file; "" = find it under /sys/bus/mipi-dsi/devices
    Q_PROPERTY(QString path READ path WRITE setPath NOTIFY pathChanged)
    Q_PROPERTY(bool available READ available NOTIFY availableChanged)
    Q_PROPERTY(int current READ current NOTIFY currentChanged)
    Q_PROPERTY(QString rates READ rates NOTIFY ratesChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY lastErrorChanged)

public:
    explicit PanelRate(QObject *parent = nullptr);
    ~PanelRate() override;

    QString path() const { return m_path; }
    void setPath(const QString &path);
    bool available() const { return !m_resolved.isEmpty(); }
    int current() const { return m_current; }
    QString rates() const { return m_rates; }
    QString lastError() const { return m_lastError; }

    // Queue a write of hz (asynchronous; current follows when done).
    Q_INVOKABLE bool setRate(int hz);
    // Re-read panel_rate_hz and panel_rates (e.g. after a modeset).
    Q_INVOKABLE void refresh();
    // For the test: wait until the queued write is done.
    Q_INVOKABLE bool waitIdle(int timeoutMs = 2000);

    static QString findAttribute();

Q_SIGNALS:
    void pathChanged();
    void availableChanged();
    void currentChanged();
    void ratesChanged();
    void lastErrorChanged();

private:
    void resolve();
    void onWritten(int hz, const QString &error);

    QString m_path;
    QString m_resolved;
    int m_current = 0;
    QString m_rates;
    QString m_lastError;
    PanelRateWriter *m_writer = nullptr;
};

class PanelRateWriter : public QThread
{
    Q_OBJECT
public:
    explicit PanelRateWriter(QObject *parent = nullptr) : QThread(parent) {}
    void post(const QString &file, int hz);
    void stop();
    bool waitIdle(int timeoutMs);

Q_SIGNALS:
    void written(int hz, const QString &error);

protected:
    void run() override;

private:
    QMutex m_lock;
    QWaitCondition m_cond;
    QWaitCondition m_idle;
    QString m_file;
    int m_pending = 0;
    bool m_busy = false;
    bool m_stop = false;
};
