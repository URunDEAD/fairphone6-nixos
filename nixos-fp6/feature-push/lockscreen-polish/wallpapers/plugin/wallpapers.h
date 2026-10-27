// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <QFileSystemWatcher>
#include <QFutureWatcher>
#include <QObject>
#include <QString>

// The lock-screen wallpaper lives in the user's LightDM greeter data
// directory ($XDG_GREETER_DATA_DIR, /var/lib/lightdm-data/<user>: owner the
// user, group lightdm, 0770), so both the session's lock screen and the
// LightDM greeter can read it, like the stock "shareWithGreeter" copies.
// It is a re-encoded copy (EXIF orientation applied, metadata dropped,
// at most 2560 px) named fp6-lock-wallpaper-<msecs>.<jpg|png>: a new name per
// change, so the shell's image cache never shows an old one. Lomiri watches
// the directory (qml/Shell.qml, lomiri-wallpapers.patch). No file = the lock
// screen shows the home wallpaper.
class Fp6Wallpapers : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString lockWallpaper READ lockWallpaper NOTIFY lockWallpaperChanged)
    Q_PROPERTY(bool hasLockWallpaper READ hasLockWallpaper NOTIFY lockWallpaperChanged)
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    Q_PROPERTY(QString greeterDataDir READ greeterDataDir CONSTANT)
    Q_PROPERTY(QString picturesUrl READ picturesUrl CONSTANT)
public:
    explicit Fp6Wallpapers(QObject *parent = nullptr);

    QString lockWallpaper() const { return m_lock; }
    bool hasLockWallpaper() const { return !m_lock.isEmpty(); }
    bool busy() const { return m_busy; }
    QString greeterDataDir() const { return m_dir; }
    QString picturesUrl() const;

    // Copies `source` (file URL or path) as the lock wallpaper. Async;
    // emits finished(ok, message).
    Q_INVOKABLE void setLockWallpaper(const QString &source);
    // Lock screen follows the home wallpaper again.
    Q_INVOKABLE void clearLockWallpaper();
    // `url` when it is an existing local file, else `fallback`.
    Q_INVOKABLE QString existingOr(const QString &url, const QString &fallback) const;

Q_SIGNALS:
    void lockWallpaperChanged();
    void busyChanged();
    void finished(bool ok, const QString &message);

private:
    void rescan();
    void setBusy(bool b);
    QString m_dir;
    QString m_lock;
    bool m_busy = false;
    QFileSystemWatcher m_watcher;
    QFutureWatcher<QString> m_job;
};
