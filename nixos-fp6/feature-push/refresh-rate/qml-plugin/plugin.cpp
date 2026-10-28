// SPDX-License-Identifier: GPL-3.0-only
// FP6.RefreshRate: the panel's runtime rate (kernel panel_rate_hz) and a
// touch/input activity watcher, for Lomiri's Fp6RefreshRate.qml.
#include <QtQml/QQmlExtensionPlugin>
#include <QtQml/qqml.h>

#include "activitywatcher.h"
#include "panelrate.h"

class RefreshRatePlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<PanelRate>(uri, 1, 0, "PanelRate");
        qmlRegisterType<ActivityWatcher>(uri, 1, 0, "ActivityWatcher");
    }
};

#include "plugin.moc"
