// SPDX-License-Identifier: GPL-3.0-only
#include "qrscanfilter.h"
#include "../common/fp6_wifi_qr.h"

#include <QDateTime>
#include <QFutureWatcher>
#include <QMetaObject>
#include <QVideoFrame>
#include <QtConcurrent/QtConcurrentRun>
#include <cstring>

namespace {

// Frames larger than this (long side) are subsampled 2:1 before decoding.
const int kMaxSide = 1600;

class Runnable : public QVideoFilterRunnable
{
public:
    explicit Runnable(QrScanFilter *f) : m_filter(f) {}

    QVideoFrame run(QVideoFrame *input, const QVideoSurfaceFormat &, RunFlags) override
    {
        if (!input->isValid() || !m_filter->wantFrame())
            return *input;
        if (input->handleType() != QAbstractVideoBuffer::NoHandle || !input->map(QAbstractVideoBuffer::ReadOnly)) {
            m_filter->noteUnreadable();
            return *input;
        }
        QByteArray luma;
        int w = 0, h = 0;
        const bool ok = extract(*input, &luma, &w, &h);
        input->unmap();
        if (ok)
            m_filter->submitLuma(luma, w, h);
        else
            m_filter->noteUnreadable();
        return *input;
    }

private:
    // Greyscale (luma) copy of a mapped frame, subsampled if large.
    static bool extract(const QVideoFrame &f, QByteArray *out, int *ow, int *oh)
    {
        const int W = f.width(), H = f.height();
        if (W <= 0 || H <= 0)
            return false;
        const int step = std::max(W, H) > kMaxSide ? 2 : 1;
        const int w = W / step, h = H / step;
        out->resize(w * h);
        auto *dst = reinterpret_cast<uchar *>(out->data());
        const uchar *src = f.bits(0);
        const int bpl = f.bytesPerLine(0);
        if (!src || bpl <= 0)
            return false;
        // bytes per pixel and luma offset, or a colour conversion
        int bpp = 0, off = 0;
        enum { Plain, Rgb32, Bgr32, Rgb24, Bgr24, Rgb565 } kind = Plain;
        switch (f.pixelFormat()) {
        case QVideoFrame::Format_YUV420P: case QVideoFrame::Format_YV12:
        case QVideoFrame::Format_NV12: case QVideoFrame::Format_NV21:
        case QVideoFrame::Format_IMC1: case QVideoFrame::Format_IMC2:
        case QVideoFrame::Format_IMC3: case QVideoFrame::Format_IMC4:
        case QVideoFrame::Format_YUV444: case QVideoFrame::Format_Y8:
            bpp = 1; break;
        case QVideoFrame::Format_YUYV: bpp = 2; off = 0; break;
        case QVideoFrame::Format_UYVY: bpp = 2; off = 1; break;
        case QVideoFrame::Format_Y16: bpp = 2; off = 1; break;  // little-endian high byte
        case QVideoFrame::Format_RGB32: case QVideoFrame::Format_ARGB32:
        case QVideoFrame::Format_ARGB32_Premultiplied:
            bpp = 4; kind = Rgb32; break;            // bytes B G R A
        case QVideoFrame::Format_BGR32: case QVideoFrame::Format_BGRA32:
        case QVideoFrame::Format_BGRA32_Premultiplied: case QVideoFrame::Format_ABGR32:
            bpp = 4; kind = Bgr32; break;            // bytes R G B A (approx. for ABGR)
        case QVideoFrame::Format_RGB24: bpp = 3; kind = Rgb24; break;
        case QVideoFrame::Format_BGR24: bpp = 3; kind = Bgr24; break;
        case QVideoFrame::Format_RGB565: bpp = 2; kind = Rgb565; break;
        default:
            return false;
        }
        for (int y = 0; y < h; ++y) {
            const uchar *row = src + qptrdiff(y * step) * bpl;
            uchar *d = dst + y * w;
            if (kind == Plain && bpp == 1 && step == 1) {
                std::memcpy(d, row, size_t(w));
                continue;
            }
            for (int x = 0; x < w; ++x) {
                const uchar *p = row + qptrdiff(x * step) * bpp;
                switch (kind) {
                case Plain: d[x] = p[off]; break;
                case Rgb32: case Rgb24: d[x] = uchar((p[2] * 77 + p[1] * 150 + p[0] * 29) >> 8); break;
                case Bgr32: case Bgr24: d[x] = uchar((p[0] * 77 + p[1] * 150 + p[2] * 29) >> 8); break;
                case Rgb565: { const int v = p[0] | (p[1] << 8);
                    d[x] = uchar((((v >> 11) & 31) * 8 * 77 + ((v >> 5) & 63) * 4 * 150 + (v & 31) * 8 * 29) >> 8); break; }
                }
            }
        }
        *ow = w;
        *oh = h;
        return true;
    }

    QrScanFilter *m_filter;
};

QByteArray decodeLuma(QByteArray luma, int w, int h)
{
    QByteArray out(4096, '\0');
    const ssize_t n = fp6_wifiqr_decode_luma(reinterpret_cast<const uint8_t *>(luma.constData()), uint32_t(w), uint32_t(h),
                                             uint32_t(w), reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()));
    if (n <= 0 || n > out.size())
        return QByteArray();
    out.truncate(int(n));
    return out;
}

} // namespace

QrScanFilter::QrScanFilter(QObject *parent)
    : QAbstractVideoFilter(parent)
{
}

QVideoFilterRunnable *QrScanFilter::createFilterRunnable()
{
    return new Runnable(this);
}

void QrScanFilter::setScanning(bool s)
{
    if (s == m_scanning)
        return;
    m_scanning = s;
    Q_EMIT scanningChanged();
}

void QrScanFilter::setInterval(int ms)
{
    ms = std::max(30, ms);
    if (ms == m_interval)
        return;
    m_interval = ms;
    Q_EMIT intervalChanged();
}

bool QrScanFilter::wantFrame()
{
    if (!m_scanning || m_busy)
        return false;
    return QDateTime::currentMSecsSinceEpoch() - m_lastStart >= m_interval;
}

void QrScanFilter::noteUnreadable()
{
    m_lastStart = QDateTime::currentMSecsSinceEpoch();
    if (++m_unreadable == 8)
        QMetaObject::invokeMethod(this, [this] {
            if (m_frameAccess) { m_frameAccess = false; Q_EMIT frameAccessChanged(); }
        }, Qt::QueuedConnection);
}

void QrScanFilter::submitLuma(QByteArray luma, int width, int height)
{
    m_busy = true;
    m_lastStart = QDateTime::currentMSecsSinceEpoch();
    m_unreadable = 0;
    // to the GUI thread; dropped if the filter is gone
    QMetaObject::invokeMethod(this, [this, luma, width, height] {
        if (!m_frameAccess) { m_frameAccess = true; Q_EMIT frameAccessChanged(); }
        startDecode(luma, width, height);
    }, Qt::QueuedConnection);
}

void QrScanFilter::decodeImage(const QVariant &image)
{
    if (!m_scanning || m_busy)
        return;
    QImage img = image.value<QImage>();
    if (img.isNull())
        return;
    if (std::max(img.width(), img.height()) > kMaxSide)
        img = img.scaled(img.width() / 2, img.height() / 2, Qt::IgnoreAspectRatio, Qt::FastTransformation);
    img = img.convertToFormat(QImage::Format_Grayscale8);
    QByteArray luma(img.width() * img.height(), '\0');
    for (int y = 0; y < img.height(); ++y)
        std::memcpy(luma.data() + y * img.width(), img.constScanLine(y), size_t(img.width()));
    m_busy = true;
    m_lastStart = QDateTime::currentMSecsSinceEpoch();
    startDecode(luma, img.width(), img.height());
}

void QrScanFilter::startDecode(QByteArray luma, int width, int height)
{
    auto *w = new QFutureWatcher<QByteArray>(this);
    connect(w, &QFutureWatcher<QByteArray>::finished, this, [this, w] {
        QByteArray text = w->result();
        w->deleteLater();
        m_busy = false;
        ++m_framesDecoded;
        Q_EMIT framesDecodedChanged();
        if (!text.isEmpty() && m_scanning)
            Q_EMIT decoded(QString::fromUtf8(text));
        fp6qr::wipe(text);
    });
    w->setFuture(QtConcurrent::run(decodeLuma, luma, width, height));
}
