// SPDX-License-Identifier: GPL-3.0-only
// Fp6.Wallpapers: separate lock-screen wallpaper for Lomiri System Settings.
#include <QQmlExtensionPlugin>
#include <QtQml>
#include "wallpapers.h"

class Fp6WallpapersPlugin : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<Fp6Wallpapers>(uri, 1, 0, "Fp6Wallpapers");
    }
};

#include "plugin.moc"
