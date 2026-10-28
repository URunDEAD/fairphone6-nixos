// SPDX-License-Identifier: GPL-3.0-only
#include "panelrate.h"

#include <QDeadlineTimer>
#include <QDir>
#include <QFile>
#include <QFileInfo>

#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <unistd.h>

static QString readFirstLine(const QString &file)
{
    QFile f(file);
    if (!f.open(QIODevice::ReadOnly))
        return QString();
    return QString::fromUtf8(f.readLine()).trimmed();
}

PanelRate::PanelRate(QObject *parent)
    : QObject(parent)
    , m_writer(new PanelRateWriter(this))
{
    connect(m_writer, &PanelRateWriter::written, this, &PanelRate::onWritten,
            Qt::QueuedConnection);
    m_writer->start(QThread::LowPriority);
    resolve();
}

PanelRate::~PanelRate()
{
    m_writer->stop();
    m_writer->wait();
}

QString PanelRate::findAttribute()
{
    QDir dir(QStringLiteral("/sys/bus/mipi-dsi/devices"));
    const auto entries = dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot, QDir::Name);
    for (const QString &e : entries) {
        const QString f = dir.filePath(e + QStringLiteral("/panel_rate_hz"));
        if (QFileInfo::exists(f))
            return f;
    }
    return QString();
}

void PanelRate::setPath(const QString &path)
{
    if (path == m_path)
        return;
    m_path = path;
    Q_EMIT pathChanged();
    resolve();
}

void PanelRate::resolve()
{
    const bool was = available();
    QString f = m_path.isEmpty() ? findAttribute() : m_path;
    // Only usable if this user may write it (udev rule: group video, 0664).
    if (!f.isEmpty() && ::access(QFile::encodeName(f).constData(), W_OK) != 0)
        f.clear();
    m_resolved = f;
    if (was != available())
        Q_EMIT availableChanged();
    refresh();
}

void PanelRate::refresh()
{
    int cur = 0;
    QString rates;
    if (available()) {
        cur = readFirstLine(m_resolved).toInt();
        rates = readFirstLine(QFileInfo(m_resolved).dir().filePath(QStringLiteral("panel_rates")));
    }
    if (cur != m_current) {
        m_current = cur;
        Q_EMIT currentChanged();
    }
    if (rates != m_rates) {
        m_rates = rates;
        Q_EMIT ratesChanged();
    }
}

bool PanelRate::setRate(int hz)
{
    if (!available() || hz <= 0)
        return false;
    m_writer->post(m_resolved, hz);
    return true;
}

bool PanelRate::waitIdle(int timeoutMs)
{
    return m_writer->waitIdle(timeoutMs);
}

void PanelRate::onWritten(int hz, const QString &error)
{
    if (error != m_lastError) {
        m_lastError = error;
        Q_EMIT lastErrorChanged();
    }
    if (error.isEmpty() && hz != m_current) {
        m_current = hz;
        Q_EMIT currentChanged();
    }
}

void PanelRateWriter::post(const QString &file, int hz)
{
    QMutexLocker l(&m_lock);
    m_file = file;
    m_pending = hz;
    m_cond.wakeOne();
}

void PanelRateWriter::stop()
{
    QMutexLocker l(&m_lock);
    m_stop = true;
    m_cond.wakeOne();
}

bool PanelRateWriter::waitIdle(int timeoutMs)
{
    QDeadlineTimer deadline(timeoutMs);
    QMutexLocker l(&m_lock);
    while (m_pending != 0 || m_busy) {
        if (!m_idle.wait(&m_lock, deadline))
            return false;
    }
    return true;
}

void PanelRateWriter::run()
{
    for (;;) {
        QString file;
        int hz;
        {
            QMutexLocker l(&m_lock);
            while (!m_stop && m_pending == 0) {
                m_idle.wakeAll();
                m_cond.wait(&m_lock);
            }
            if (m_stop)
                return;
            file = m_file;
            hz = m_pending;
            m_pending = 0;
            m_busy = true;
        }

        // One write(2) of the decimal value: sysfs store gets it whole
        // (O_TRUNC as with "echo >"; kernfs ignores it).
        QString error;
        const QByteArray value = QByteArray::number(hz) + '\n';
        int fd = ::open(QFile::encodeName(file).constData(), O_WRONLY | O_TRUNC | O_CLOEXEC);
        if (fd < 0) {
            error = QString::fromLocal8Bit(std::strerror(errno));
        } else {
            if (::write(fd, value.constData(), value.size()) != value.size())
                error = QString::fromLocal8Bit(std::strerror(errno));
            ::close(fd);
        }
        Q_EMIT written(hz, error);

        QMutexLocker l(&m_lock);
        m_busy = false;
        if (m_pending == 0)
            m_idle.wakeAll();
    }
}
