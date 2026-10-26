#include "sensorfeed.h"
#include "fp6_sensors.h"

#include <QJsonDocument>
#include <QJsonObject>

static QVariantMap parse(char *s)
{
    QVariantMap m;
    if (s) {
        m = QJsonDocument::fromJson(QByteArray(s)).object().toVariantMap();
        fp6_string_free(s);
    }
    return m;
}

SensorFeed::SensorFeed(QObject *parent) : QObject(parent)
{
    m_timer.setInterval(50);
    connect(&m_timer, &QTimer::timeout, this, &SensorFeed::poll);
}

SensorFeed::~SensorFeed()
{
    if (m_feed)
        fp6_feed_free(m_feed);  // stops and joins the worker
}

void SensorFeed::setKind(const QString &k)
{
    if (k == m_kind)
        return;
    if (m_feed) {
        fp6_feed_free(m_feed);
        m_feed = nullptr;
    }
    m_kind = k;
    m_feed = fp6_feed_new(k.toUtf8().constData());
    m_data.clear();
    emit kindChanged();
    apply();
}

void SensorFeed::setActive(bool on)
{
    if (on == m_active)
        return;
    m_active = on;
    emit activeChanged();
    apply();
}

void SensorFeed::setInterval(int ms)
{
    if (ms == m_timer.interval() || ms < 10)
        return;
    m_timer.setInterval(ms);
    emit intervalChanged();
}

void SensorFeed::apply()
{
    if (!m_feed)
        return;
    fp6_feed_set_active(m_feed, m_active);
    if (m_active)
        m_timer.start();
    else
        m_timer.stop();
}

void SensorFeed::poll()
{
    if (!m_feed)
        return;
    char *s = fp6_feed_take(m_feed);
    if (!s)
        return;
    m_data = parse(s);
    emit updated();
}

QVariantMap SensorFeed::command(const QString &name, const QString &arg)
{
    if (!m_feed)
        return {};
    return parse(fp6_feed_command(m_feed, name.toUtf8().constData(), arg.toUtf8().constData()));
}
