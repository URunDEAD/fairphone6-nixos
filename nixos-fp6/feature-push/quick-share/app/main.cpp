// SPDX-License-Identifier: GPL-3.0-only
#include <QGuiApplication>
#include <QIcon>
#include <QClipboard>
#include <QCommandLineParser>
#include <QDBusConnection>
#include <QDBusVariant>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDesktopServices>
#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QQmlContext>
#include <QQuickView>
#include <QTimer>
#include <QUrl>

static const QString bus = QStringLiteral("com.fairphone6.QuickShare1");
static const QString path = QStringLiteral("/com/fairphone6/QuickShare1");

class Bridge : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantList devices READ devices NOTIFY changed)
    Q_PROPERTY(QVariantList transfers READ transfers NOTIFY changed)
    Q_PROPERTY(QString visibility READ visibility NOTIFY changed)
    Q_PROPERTY(QString deviceName READ deviceName NOTIFY changed)
    Q_PROPERTY(QString error READ error NOTIFY changed)
    Q_PROPERTY(QStringList selectedUris READ selectedUris NOTIFY changed)
    Q_PROPERTY(bool preview READ preview CONSTANT)
    Q_PROPERTY(bool startOnSend READ startOnSend CONSTANT)
public:
    Bridge(bool preview, QStringList uris, QString showTransfer, QString action, bool sendPreview) : m_preview(preview), m_uris(uris), m_showTransfer(showTransfer), m_action(action), m_sendPreview(sendPreview) {
        if (m_preview) {
            m_name = "Fairphone 6";
            if (!sendPreview) m_visibility = "everyone-for-10-min";
            m_devices = {QVariantMap{{"id", "synthetic"}, {"name", "Android phone"}}};
            if (!sendPreview) m_transfers = {QVariantMap{{"id", "synthetic-request"}, {"sender", "Android phone"}, {"direction", "inbound"}, {"files", QStringList{"Weekend photo.jpg", "Travel notes.txt"}}, {"pin", "2846"}, {"status", "waiting"}, {"bytes", 0}, {"total", 2457600}}};
        } else {
            auto connection = QDBusConnection::sessionBus();
            connection.connect(bus, path, bus, "DeviceFound", this, SLOT(found(QString,QString)));
            connection.connect(bus, path, bus, "DeviceLost", this, SLOT(lost(QString)));
            connection.connect(bus, path, bus, "IncomingRequest", this, SLOT(incoming(QString,QString,QStringList,QString)));
            connection.connect(bus, path, bus, "Progress", this, SLOT(progress(QString,qulonglong,qulonglong)));
            connection.connect(bus, path, bus, "Finished", this, SLOT(finished(QString,QString)));
            auto timer = new QTimer(this);
            connect(timer, &QTimer::timeout, this, &Bridge::refresh);
            timer->start(1000); refresh();
        }
    }
    QVariantList devices() const { return m_devices; }
    QVariantList transfers() const { return m_transfers; }
    QString visibility() const { return m_visibility; }
    QString deviceName() const { return m_name; }
    QString error() const { return m_error; }
    QStringList selectedUris() const { return m_uris; }
    bool preview() const { return m_preview; }
    bool startOnSend() const { return m_sendPreview || !m_uris.isEmpty(); }
    Q_INVOKABLE void setVisible(bool value) {
        if (m_preview) { m_visibility = value ? "everyone-for-10-min" : "off"; emit changed(); return; }
        call("org.freedesktop.DBus.Properties", "Set", {bus, "Visibility", QVariant::fromValue(QDBusVariant(value ? QString("everyone-for-10-min") : QString("off")))});
    }
    Q_INVOKABLE void discover() { if (!m_preview) call(bus, "StartDiscovery"); }
    Q_INVOKABLE void decide(const QString &id, bool accept) {
        if (m_preview) { for (auto &value : m_transfers) { auto t = value.toMap(); if (t["id"] == id) { t["status"] = accept ? "receiving" : "rejected"; value = t; } } emit changed(); return; }
        call(bus, accept ? "Accept" : "Reject", {id});
    }
    Q_INVOKABLE void cancel(const QString &id) { if (!m_preview) call(bus, "Cancel", {id}); }
    Q_INVOKABLE void setUris(const QVariantList &values) {
        m_uris.clear();
        for (const auto &value : values) {
            const QString raw = value.toString();
            const QUrl url(raw);
            if (raw.startsWith("text:") || raw.startsWith("url:")) m_uris.append(raw);
            else if (url.isLocalFile()) m_uris.append(url.toString());
            else m_error = "Choose local files, text or a web link.";
        }
        emit changed();
    }
    Q_INVOKABLE void clearSelection() { m_uris.clear(); emit changed(); }
    Q_INVOKABLE void send(const QString &id, const QString &text, bool link) {
        QStringList items = m_uris;
        if (!text.isEmpty()) items = QStringList{QString(link ? "url:" : "text:") + text};
        if (m_preview) return;
        call(bus, "Send", {id, items});
    }
    Q_INVOKABLE void copyText(const QString &text) { QGuiApplication::clipboard()->setText(text); }
    Q_INVOKABLE void openTransfer(const QVariantMap &transfer) {
        if (transfer.value("status") != "finished") return;
        if (transfer.value("is_url").toBool()) {
            const QUrl url(transfer.value("text").toString());
            if (url.scheme() == "https" || url.scheme() == "http") QDesktopServices::openUrl(url);
        } else if (!transfer.value("directory").toString().isEmpty()) QDesktopServices::openUrl(QUrl::fromLocalFile(transfer.value("directory").toString()));
    }
public slots:
    void refresh() {
        if (m_preview || m_refreshing) return;
        m_refreshing = true;
        auto message = QDBusMessage::createMethodCall(bus, path, bus, "Snapshot");
        auto watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(message, 3000), this);
        connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher] {
            QDBusPendingReply<QString> reply = *watcher;
            m_refreshing = false;
            if (reply.isError()) m_error = "Quick Share service unavailable: " + reply.error().message();
            else {
                const auto snapshot = QJsonDocument::fromJson(reply.value().toUtf8()).object();
                m_devices = snapshot["devices"].toArray().toVariantList();
                m_transfers = snapshot["transfers"].toArray().toVariantList();
                m_visibility = snapshot["visibility"].toString(); m_name = snapshot["deviceName"].toString();
                if (m_error.startsWith("Quick Share service unavailable:")) m_error.clear();
                if (!m_action.isEmpty()) for (const auto &value : m_transfers) {
                    const auto t = value.toMap();
                    if (t["id"].toString() == m_showTransfer && t["status"] == "finished") {
                        if (m_action == "copy") copyText(t["text"].toString());
                        if (m_action == "open") openTransfer(t);
                        m_action.clear(); break;
                    }
                }
            }
            emit changed(); watcher->deleteLater();
        });
    }
    void found(QString, QString) { refresh(); }
    void lost(QString) { refresh(); }
    void incoming(QString, QString, QStringList, QString) { refresh(); }
    void progress(QString, qulonglong, qulonglong) { refresh(); }
    void finished(QString, QString) { refresh(); }
signals:
    void changed();
private:
    void call(const QString &interface, const QString &method, const QVariantList &arguments = {}) {
        auto message = QDBusMessage::createMethodCall(bus, path, interface, method);
        message.setArguments(arguments);
        auto watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(message, 15000), this);
        connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher] {
            if (watcher->isError()) { m_error = watcher->error().message(); emit changed(); }
            else { m_error.clear(); refresh(); }
            watcher->deleteLater();
        });
    }
    bool m_preview, m_refreshing = false;
    QStringList m_uris;
    QString m_visibility = "off", m_name = "Fairphone 6", m_error, m_showTransfer, m_action;
    bool m_sendPreview;
    QVariantList m_devices, m_transfers;
};

int main(int argc, char **argv) {
    if (qEnvironmentVariableIsEmpty("APP_ID")) qputenv("APP_ID", "com.fairphone6.QuickShare");
    QGuiApplication app(argc, argv);
    QIcon::setThemeName("suru");
    app.setDesktopFileName("com.fairphone6.QuickShare");
    app.setApplicationName("fp6-quickshare");
    app.setOrganizationName("fairphone6");
    QCommandLineParser parser;
    parser.addHelpOption();
    parser.addOptions({{"preview", "Preview synthetic UI without contacting the daemon."}, {"preview-send", "Preview the send screen."}, {"screenshot", "Save offscreen screenshot and exit.", "path"}, {"show-transfer", "Show transfer from notification.", "id"}, {"copy", "Copy received text after explicit notification action."}, {"open", "Open received content after explicit notification action."}});
    parser.addPositionalArgument("uris", "Local file paths or file:// URIs to share.", "[uris…]");
    parser.process(app);
    QStringList uris;
    for (const auto &value : parser.positionalArguments()) {
        auto url = QUrl::fromUserInput(value, QDir::currentPath(), QUrl::AssumeLocalFile);
        if (url.isLocalFile()) uris.append(url.toString());
    }
    Bridge bridge(parser.isSet("preview") || parser.isSet("preview-send"), uris, parser.value("show-transfer"), parser.isSet("copy") ? "copy" : parser.isSet("open") ? "open" : "", parser.isSet("preview-send"));
    QQuickView view;
    view.rootContext()->setContextProperty("quickShare", &bridge);
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    const auto installed = QCoreApplication::applicationDirPath() + "/../share/fp6-quickshare/qml/Main.qml";
    view.setSource(QUrl::fromLocalFile(QFileInfo::exists(installed) ? installed : QStringLiteral(QML_SOURCE_DIR "/Main.qml")));
    if (view.status() == QQuickView::Error) return 1;
    view.resize(432, 820); view.setTitle("Quick Share"); view.show();
    if (parser.isSet("screenshot")) QTimer::singleShot(1800, &app, [&] { app.exit(view.grabWindow().save(parser.value("screenshot")) ? 0 : 2); });
    return app.exec();
}
#include "main.moc"
