// SPDX-License-Identifier: GPL-3.0-only
#include <QtQml/QQmlExtensionPlugin>
#include <QtQml/qqml.h>

#include "colorfilter.h"

class DisplayComfortPlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<ColorFilter>(uri, 1, 0, "ColorFilter");
    }
};

#include "plugin.moc"
