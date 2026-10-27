/*
 * Fp6.StorageBackup: the one bridge the Storage and Backup settings pages
 * need. StorageAgent.call(method, jsonArgs, callback(error, jsonResult))
 * forwards to fp6-storage-agent on the session bus
 * (com.fairphone6.StorageBackup1.Call, D-Bus activated); "changed" relays
 * its Changed(s) signal. No logic here: everything else is in Rust.
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

static const char *kService = "com.fairphone6.StorageBackup";
static const char *kPath = "/com/fairphone6/StorageBackup";
static const char *kIface = "com.fairphone6.StorageBackup1";

class StorageAgent : public QObject
{
    Q_OBJECT
public:
    explicit StorageAgent(QObject *parent = nullptr) : QObject(parent)
    {
        QDBusConnection::sessionBus().connect(kService, kPath, kIface, "Changed", this, SLOT(onChanged(QString)));
    }

    // Long calls (passphrase key derivation, opening an archive) get 2 minutes.
    Q_INVOKABLE void call(const QString &method, const QString &args, const QJSValue &callback)
    {
        QDBusMessage m = QDBusMessage::createMethodCall(kService, kPath, kIface, "Call");
        m << method << args;
        auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(m, 120000), this);
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

class Fp6StorageBackupPlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<StorageAgent>(uri, 1, 0, "StorageAgent");
    }
};

#include "plugin.moc"
