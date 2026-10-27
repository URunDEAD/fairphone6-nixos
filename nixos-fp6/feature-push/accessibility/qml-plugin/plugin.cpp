// SPDX-License-Identifier: GPL-3.0-only
#include <QtQml/QQmlEngine>
#include <QtQml/QQmlExtensionPlugin>
#include <QtQml/qqml.h>

#include "colourmatrix.h"
#include "magnifier.h"

class AccessibilityPlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<ColourMatrix>(uri, 1, 0, "ColourMatrix");
        qmlRegisterType<Magnifier>(uri, 1, 0, "Magnifier");
        qmlRegisterSingletonType<MagnifierControl>(uri, 1, 0, "MagnifierControl",
            [](QQmlEngine *, QJSEngine *) -> QObject * {
                auto *c = MagnifierControl::instance();
                QQmlEngine::setObjectOwnership(c, QQmlEngine::CppOwnership);
                return c;
            });
    }
};

#include "plugin.moc"
