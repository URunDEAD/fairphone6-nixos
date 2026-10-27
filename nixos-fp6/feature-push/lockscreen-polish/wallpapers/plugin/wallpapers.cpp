// SPDX-License-Identifier: GPL-3.0-only
#include "wallpapers.h"

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QStandardPaths>
#include <QUrl>
#include <QtConcurrent>
#include <unistd.h>
#include <pwd.h>

namespace {
const char kPrefix[] = "fp6-lock-wallpaper-";
const int kMaxSide = 2560;

QString localPath(const QString &s)
{
    const QUrl u(s);
    if (u.isLocalFile()) return u.toLocalFile();
    if (u.scheme().isEmpty()) return s;
    return QString();
}

QStringList ours(const QDir &dir)
{
    return dir.entryList({ QString(kPrefix) + "*" }, QDir::Files | QDir::NoSymLinks, QDir::Name);
}

// Runs in a worker thread. Returns "" on success, else an error message.
QString writeCopy(const QString &dirPath, const QString &src)
{
    QImageReader reader(src);
    reader.setAutoTransform(true);
    QSize size = reader.size();
    if (size.isValid() && qMax(size.width(), size.height()) > kMaxSide) {
        size.scale(kMaxSide, kMaxSide, Qt::KeepAspectRatio);
        reader.setScaledSize(size);
    }
    QImage img = reader.read();
    if (img.isNull()) return QStringLiteral("cannot read image: ") + reader.errorString();

    QDir dir(dirPath);
    if (!dir.exists()) return QStringLiteral("greeter data directory missing: ") + dirPath;
    const bool alpha = img.hasAlphaChannel();
    const QString name = QString(kPrefix) + QString::number(QDateTime::currentMSecsSinceEpoch())
                         + (alpha ? ".png" : ".jpg");
    // Written under a hidden name, then renamed: the shell (which lists
    // fp6-lock-wallpaper-*) never sees a partial file.
    const QString tmp = dir.filePath(QStringLiteral(".fp6-lock-wallpaper.tmp"));
    QFile::remove(tmp);
    {
        QFile out(tmp);
        if (!out.open(QIODevice::WriteOnly)) return QStringLiteral("cannot write ") + tmp;
        if (!img.save(&out, alpha ? "PNG" : "JPG", alpha ? -1 : 92) || !out.flush()) {
            out.close();
            QFile::remove(tmp);
            return QStringLiteral("cannot encode image");
        }
        fsync(out.handle());
    }
    if (!QFile::rename(tmp, dir.filePath(name))) {
        QFile::remove(tmp);
        return QStringLiteral("cannot write ") + dir.filePath(name);
    }
    // Readable by the greeter (group lightdm can only traverse the 0770 dir).
    QFile::setPermissions(dir.filePath(name), QFileDevice::ReadOwner | QFileDevice::WriteOwner
                                              | QFileDevice::ReadGroup | QFileDevice::ReadOther);
    for (const QString &old : ours(dir))
        if (old != name) dir.remove(old);
    return QString();
}
} // namespace

Fp6Wallpapers::Fp6Wallpapers(QObject *parent) : QObject(parent)
{
    m_dir = qEnvironmentVariable("XDG_GREETER_DATA_DIR");
    if (m_dir.isEmpty()) {
        const struct passwd *pw = getpwuid(getuid());
        m_dir = QStringLiteral(FP6_GREETER_DATA_DIR) + "/" + QString::fromLocal8Bit(pw ? pw->pw_name : "");
    }
    if (QFileInfo(m_dir).isDir()) m_watcher.addPath(m_dir);
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, &Fp6Wallpapers::rescan);
    connect(&m_job, &QFutureWatcher<QString>::finished, this, [this]() {
        const QString err = m_job.result();
        rescan();
        setBusy(false);
        if (!err.isEmpty()) qWarning("Fp6Wallpapers: %s", qPrintable(err));
        Q_EMIT finished(err.isEmpty(), err);
    });
    rescan();
}

void Fp6Wallpapers::rescan()
{
    const QStringList files = ours(QDir(m_dir));
    const QString now = files.isEmpty() ? QString()
                                        : QUrl::fromLocalFile(QDir(m_dir).filePath(files.last())).toString();
    if (now != m_lock) {
        m_lock = now;
        Q_EMIT lockWallpaperChanged();
    }
}

void Fp6Wallpapers::setBusy(bool b)
{
    if (b == m_busy) return;
    m_busy = b;
    Q_EMIT busyChanged();
}

void Fp6Wallpapers::setLockWallpaper(const QString &source)
{
    const QString src = localPath(source);
    if (src.isEmpty() || !QFileInfo(src).isFile()) {
        Q_EMIT finished(false, QStringLiteral("not a local file: ") + source);
        return;
    }
    if (m_busy) return;
    setBusy(true);
    m_job.setFuture(QtConcurrent::run(writeCopy, m_dir, src));
}

void Fp6Wallpapers::clearLockWallpaper()
{
    QDir dir(m_dir);
    for (const QString &f : ours(dir)) dir.remove(f);
    rescan();
}

QString Fp6Wallpapers::existingOr(const QString &url, const QString &fallback) const
{
    const QString p = localPath(url);
    return (!p.isEmpty() && QFileInfo(p).isFile()) ? url : fallback;
}

QString Fp6Wallpapers::picturesUrl() const
{
    return QUrl::fromLocalFile(QStandardPaths::writableLocation(QStandardPaths::PicturesLocation)).toString();
}
