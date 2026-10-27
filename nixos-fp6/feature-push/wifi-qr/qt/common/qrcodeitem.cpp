// SPDX-License-Identifier: GPL-3.0-only
#include "qrcodeitem.h"
#include "fp6_wifi_qr.h"

#include <QPainter>
#include <QQuickWindow>
#include <cmath>

static const int kQuiet = 4;

QrCodeItem::QrCodeItem(QQuickItem *parent)
    : QQuickPaintedItem(parent)
{
    setAntialiasing(false);
    setSmooth(false);
    setOpaquePainting(true);
    connect(this, &QrCodeItem::colorsChanged, this, [this] { update(); });
}

QrCodeItem::~QrCodeItem()
{
    fp6qr::wipe(m_text);
    if (!m_modules.isEmpty())
        fp6_wifiqr_wipe(m_modules.data(), size_t(m_modules.size()));
}

void QrCodeItem::setText(const QString &t)
{
    if (t == m_text)
        return;
    fp6qr::wipe(m_text);
    m_text = t;
    if (!m_modules.isEmpty())
        fp6_wifiqr_wipe(m_modules.data(), size_t(m_modules.size()));
    m_modules.clear();
    m_width = 0;
    if (!t.isEmpty()) {
        QByteArray u = t.toUtf8();
        const auto *p = reinterpret_cast<const uint8_t *>(u.constData());
        int w = fp6_wifiqr_matrix(p, size_t(u.size()), nullptr, 0);
        if (w > 0) {
            m_modules.resize(w * w);
            if (fp6_wifiqr_matrix(p, size_t(u.size()), m_modules.data(), size_t(m_modules.size())) == w)
                m_width = w;
            else
                m_modules.clear();
        }
        fp6qr::wipe(u);
    }
    Q_EMIT textChanged();
    update();
}

void QrCodeItem::paint(QPainter *painter)
{
    painter->setRenderHint(QPainter::Antialiasing, false);
    painter->fillRect(QRectF(0, 0, width(), height()), m_light);
    if (m_width <= 0)
        return;
    // Whole device pixels per module, centred: no blurred module edges.
    const qreal dpr = window() ? window()->effectiveDevicePixelRatio() : 1.0;
    const int n = m_width + 2 * kQuiet;
    const int sidePx = int(std::floor(std::min(width(), height()) * dpr));
    const int modulePx = std::max(1, sidePx / n);
    const int codePx = modulePx * n;
    const int x0 = (int(std::floor(width() * dpr)) - codePx) / 2 + kQuiet * modulePx;
    const int y0 = (int(std::floor(height() * dpr)) - codePx) / 2 + kQuiet * modulePx;
    for (int y = 0; y < m_width; ++y) {
        int x = 0;
        while (x < m_width) {
            if (!m_modules[y * m_width + x]) { ++x; continue; }
            int run = x;
            while (run < m_width && m_modules[y * m_width + run])
                ++run;
            painter->fillRect(QRectF((x0 + x * modulePx) / dpr, (y0 + y * modulePx) / dpr,
                                     (run - x) * modulePx / dpr, modulePx / dpr), m_dark);
            x = run;
        }
    }
}
