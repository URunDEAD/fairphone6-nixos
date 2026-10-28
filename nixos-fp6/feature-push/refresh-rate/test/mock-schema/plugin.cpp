// SPDX-License-Identifier: GPL-3.0-only
// Test-only QML module GSettingsMock: the "schema" object of the GSettings
// mock (a QML file cannot declare a property named "id").
#include <QtQml/QQmlExtensionPlugin>
#include <QtQml/qqml.h>

class MockSchema : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString id MEMBER m_id NOTIFY idChanged)
public:
    using QObject::QObject;
Q_SIGNALS:
    void idChanged();
private:
    QString m_id;
};

class GSettingsMockPlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<MockSchema>(uri, 1, 0, "MockSchema");
    }
};

#include "plugin.moc"
