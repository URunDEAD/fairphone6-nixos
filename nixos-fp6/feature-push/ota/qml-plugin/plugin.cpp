/*
 * Fp6.Update: the bridge the System update settings page needs.
 * UpdateService.call(method, jsonArgs, callback(error, jsonResult)) forwards
 * to fp6-updated on the system bus (com.fairphone6.Update1.Call, D-Bus
 * activated; the session bus when FP6_UPDATE_SESSION_BUS is set, for the
 * laptop previews); "changed" relays its Changed(s) signal. No logic here.
 * SPDX-License-Identifier: GPL-3.0-only
 */
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QJSValue>
#include <QObject>
#include <QQmlEngine>
#include <QQmlExtensionPlugin>

static const char *kService = "com.fairphone6.Update";
static const char *kPath = "/com/fairphone6/Update";
static const char *kIface = "com.fairphone6.Update1";

static QDBusConnection bus()
{
    return qEnvironmentVariableIsSet("FP6_UPDATE_SESSION_BUS") ? QDBusConnection::sessionBus() : QDBusConnection::systemBus();
}

class UpdateService : public QObject
{
    Q_OBJECT
public:
    explicit UpdateService(QObject *parent = nullptr) : QObject(parent)
    {
        bus().connect(kService, kPath, kIface, "Changed", this, SLOT(onChanged(QString)));
    }

    Q_INVOKABLE void call(const QString &method, const QString &args, const QJSValue &callback)
    {
        QDBusMessage m = QDBusMessage::createMethodCall(kService, kPath, kIface, "Call");
        m << method << args;
        auto *w = new QDBusPendingCallWatcher(bus().asyncCall(m, 60000), this);
        QJSValue cb = callback;
        connect(w, &QDBusPendingCallWatcher::finished, this, [cb](QDBusPendingCallWatcher *w) mutable {
            QDBusPendingReply<QString> r = *w;
            w->deleteLater();
            if (!cb.isCallable())
                return;
            if (r.isError())
                cb.call({QJSValue(r.error().message()), QJSValue()});
            else
                cb.call({QJSValue(QJSValue::NullValue), QJSValue(r.value())});
        });
    }

Q_SIGNALS:
    void changed(const QString &job);

private Q_SLOTS:
    void onChanged(const QString &job) { Q_EMIT changed(job); }
};

class Fp6UpdatePlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<UpdateService>(uri, 1, 0, "UpdateService");
    }
};

#include "plugin.moc"
