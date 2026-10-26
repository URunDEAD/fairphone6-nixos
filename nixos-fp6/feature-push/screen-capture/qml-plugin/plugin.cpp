// SPDX-License-Identifier: GPL-3.0-only
#include <QQmlExtensionPlugin>
#include <QtQml/qqml.h>

#include "screenrecorder.h"

class Fp6ScreenRecorderPlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        Q_ASSERT(uri == QLatin1String("FP6.ScreenRecorder"));
        qmlRegisterType<ScreenRecorder>(uri, 0, 1, "ScreenRecorder");
    }
};

#include "plugin.moc"
