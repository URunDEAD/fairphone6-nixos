// SensorFeed: the only C++ type, a thin QObject over the Rust feed.
//
// Why C++ at all: a Qt 5.15 QML type has to be a C++ QObject (moc). The
// shim only forwards: it starts/stops the Rust worker when `active`
// changes, polls the latest JSON snapshot on a timer and parses it into a
// QVariantMap for QML. All sensor I/O, maths and state are in Rust.
#pragma once
#include <QObject>
#include <QTimer>
#include <QVariantMap>

struct Fp6Feed;

class SensorFeed : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString kind READ kind WRITE setKind NOTIFY kindChanged)
    Q_PROPERTY(bool active READ active WRITE setActive NOTIFY activeChanged)
    Q_PROPERTY(int interval READ interval WRITE setInterval NOTIFY intervalChanged)
    Q_PROPERTY(QVariantMap data READ data NOTIFY updated)
    Q_PROPERTY(bool valid READ valid NOTIFY kindChanged)

public:
    explicit SensorFeed(QObject *parent = nullptr);
    ~SensorFeed() override;

    QString kind() const { return m_kind; }
    void setKind(const QString &k);
    bool active() const { return m_active; }
    void setActive(bool on);
    int interval() const { return m_timer.interval(); }
    void setInterval(int ms);
    QVariantMap data() const { return m_data; }
    bool valid() const { return m_feed != nullptr; }

    Q_INVOKABLE QVariantMap command(const QString &name, const QString &arg = QString());

signals:
    void kindChanged();
    void activeChanged();
    void intervalChanged();
    void updated();

private:
    void poll();
    void apply();

    QString m_kind;
    bool m_active = false;
    Fp6Feed *m_feed = nullptr;
    QTimer m_timer;
    QVariantMap m_data;
};
