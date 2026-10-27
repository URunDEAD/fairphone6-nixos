// fp6-file-picker: QML host for the Lomiri UI Toolkit file picker that
// xdg-desktop-portal-lomiri starts for every FileChooser request.
//
// Request: one JSON object on stdin (or the file named by FP6_PICKER_REQUEST).
// Answer:  one JSON line on stdout, {"response":0|1|2,"paths":[...],
//          "filter":i,"choices":[[id,value],...]}. No answer (window closed,
//          crash) means "cancelled" to the portal.
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickView>
#include <QStandardPaths>
#include <QTimer>
#include <QUrl>
#include <cstdio>

#ifndef FP6_PICKER_QML_DIR
#define FP6_PICKER_QML_DIR "qml"
#endif

class PickerHost : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QVariantMap request READ request CONSTANT)
    Q_PROPERTY(QString preview READ preview CONSTANT)
public:
    explicit PickerHost(const QVariantMap &request, QObject *parent = nullptr)
        : QObject(parent), m_request(request) {}

    QVariantMap request() const { return m_request; }
    QString preview() const { return qEnvironmentVariable("FP6_PREVIEW"); }

    // Writes the answer once and quits.
    Q_INVOKABLE void finish(const QVariantMap &answer)
    {
        if (m_done)
            return;
        m_done = true;
        const QByteArray line = QJsonDocument(QJsonObject::fromVariantMap(answer)).toJson(QJsonDocument::Compact);
        fwrite(line.constData(), 1, line.size(), stdout);
        fputc('\n', stdout);
        fflush(stdout);
        QTimer::singleShot(0, qApp, &QGuiApplication::quit);
    }
    Q_INVOKABLE bool exists(const QString &path) const { return QFileInfo::exists(path); }
    Q_INVOKABLE bool isDir(const QString &path) const { return QFileInfo(path).isDir(); }
    Q_INVOKABLE bool isWritableDir(const QString &path) const
    {
        QFileInfo fi(path);
        return fi.isDir() && fi.isWritable();
    }
    Q_INVOKABLE bool mkdir(const QString &path) const { return QDir().mkpath(path); }
    Q_INVOKABLE QString parentOf(const QString &path) const
    {
        QString p = QDir::cleanPath(path);
        return p == QLatin1String("/") ? p : QFileInfo(p).absolutePath();
    }
    Q_INVOKABLE QString join(const QString &dir, const QString &name) const
    {
        return QDir::cleanPath(dir + QLatin1Char('/') + name);
    }

private:
    QVariantMap m_request;
    bool m_done = false;
};

static QVariantMap readRequest()
{
    QByteArray data;
    const QString file = qEnvironmentVariable("FP6_PICKER_REQUEST");
    if (!file.isEmpty()) {
        QFile f(file);
        if (f.open(QIODevice::ReadOnly))
            data = f.readAll();
    } else {
        QFile in;
        if (in.open(stdin, QIODevice::ReadOnly))
            data = in.readAll();
    }
    const QJsonDocument doc = QJsonDocument::fromJson(data);
    QVariantMap req = doc.isObject() ? doc.object().toVariantMap() : QVariantMap();
    if (!req.contains(QStringLiteral("mode")))
        req.insert(QStringLiteral("mode"), QStringLiteral("open"));
    if (!req.contains(QStringLiteral("home")))
        req.insert(QStringLiteral("home"), QDir::homePath());
    return req;
}

int main(int argc, char *argv[])
{
    // Lomiri apps and the UITK expect these.
    QGuiApplication::setApplicationName(QStringLiteral("fp6-file-picker"));
    QGuiApplication::setOrganizationName(QStringLiteral("fp6-file-picker"));
    QGuiApplication app(argc, argv);
    app.setApplicationDisplayName(QStringLiteral("Files"));

    // Read the whole request before Qt prints anything (stdout is the answer
    // channel; Qt's own messages go to stderr).
    const QVariantMap req = readRequest();
    PickerHost host(req);

    const QString qmlDir = qEnvironmentVariable("FP6_PICKER_QML", QStringLiteral(FP6_PICKER_QML_DIR));
    QQuickView view;
    view.setResizeMode(QQuickView::SizeRootObjectToView);
    view.setTitle(req.value(QStringLiteral("title")).toString().isEmpty()
                      ? QStringLiteral("Files")
                      : req.value(QStringLiteral("title")).toString());
    view.rootContext()->setContextProperty(QStringLiteral("picker"), &host);
    QObject::connect(view.engine(), &QQmlEngine::quit, &app, &QGuiApplication::quit);
    view.setSource(QUrl::fromLocalFile(QDir(qmlDir).filePath(QStringLiteral("Main.qml"))));
    if (view.status() == QQuickView::Error)
        return 3;

    // Test hook for offline previews: FP6_GRAB=/path.png[:delay_ms]
    // [FP6_GRAB_SIZE=WxH] saves one frame and quits (prints no answer).
    const QString grab = qEnvironmentVariable("FP6_GRAB");
    const QStringList size = qEnvironmentVariable("FP6_GRAB_SIZE").split(QLatin1Char('x'));
    if (size.size() == 2)
        view.resize(size[0].toInt(), size[1].toInt());
    view.show();
    if (!grab.isEmpty()) {
        const QStringList g = grab.split(QLatin1Char(':'));
        QTimer::singleShot(g.size() > 1 ? g[1].toInt() : 2000, &view, [&view, g]() {
            view.grabWindow().save(g[0]);
            qApp->quit();
        });
    }
    return app.exec();
}

#include "main.moc"
