// SPDX-License-Identifier: GPL-3.0-only
// QrScanFilter: VideoOutput filter that hands a greyscale copy of a camera
// frame to the Rust decoder every `interval` ms (one decode at a time, in a
// worker thread). decodeImage() is the fallback for frames that cannot be
// mapped (GPU textures): QML grabs the VideoOutput and passes the image.
#pragma once
#include <QAbstractVideoFilter>
#include <QByteArray>
#include <QImage>
#include <QVariant>
#include <atomic>

class QrScanFilter : public QAbstractVideoFilter
{
    Q_OBJECT
    Q_PROPERTY(bool scanning READ scanning WRITE setScanning NOTIFY scanningChanged)
    Q_PROPERTY(int interval READ interval WRITE setInterval NOTIFY intervalChanged)
    // true once a camera frame could be read on the CPU; false after
    // unreadable ones (then QML uses decodeImage()).
    Q_PROPERTY(bool frameAccess READ frameAccess NOTIFY frameAccessChanged)
    Q_PROPERTY(int framesDecoded READ framesDecoded NOTIFY framesDecodedChanged)
public:
    explicit QrScanFilter(QObject *parent = nullptr);
    QVideoFilterRunnable *createFilterRunnable() override;

    bool scanning() const { return m_scanning; }
    void setScanning(bool s);
    int interval() const { return m_interval; }
    void setInterval(int ms);
    bool frameAccess() const { return m_frameAccess; }
    int framesDecoded() const { return m_framesDecoded; }

    Q_INVOKABLE void decodeImage(const QVariant &image);

    // render thread
    bool wantFrame();
    void submitLuma(QByteArray luma, int width, int height);
    void noteUnreadable();

Q_SIGNALS:
    void scanningChanged();
    void intervalChanged();
    void frameAccessChanged();
    void framesDecodedChanged();
    void decoded(const QString &text);

private:
    void startDecode(QByteArray luma, int width, int height);

    std::atomic<bool> m_scanning{true};
    std::atomic<bool> m_busy{false};
    std::atomic<qint64> m_lastStart{0};
    std::atomic<int> m_unreadable{0};
    int m_interval = 150;
    bool m_frameAccess = true;
    int m_framesDecoded = 0;
};
