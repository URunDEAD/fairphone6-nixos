// SPDX-License-Identifier: GPL-3.0-only
// QrCode { text: "..." }: a QR code drawn module by module on whole device
// pixels (crisp at any scale), black on white with the 4-module quiet zone.
#pragma once
#include <QQuickPaintedItem>
#include <QVector>

class QrCodeItem : public QQuickPaintedItem
{
    Q_OBJECT
    Q_PROPERTY(QString text READ text WRITE setText NOTIFY textChanged)
    Q_PROPERTY(int modules READ modules NOTIFY textChanged)
    Q_PROPERTY(QColor darkColor MEMBER m_dark NOTIFY colorsChanged)
    Q_PROPERTY(QColor lightColor MEMBER m_light NOTIFY colorsChanged)
public:
    explicit QrCodeItem(QQuickItem *parent = nullptr);
    ~QrCodeItem() override;
    QString text() const { return m_text; }
    void setText(const QString &t);
    int modules() const { return m_width; }
    void paint(QPainter *painter) override;
Q_SIGNALS:
    void textChanged();
    void colorsChanged();
private:
    QString m_text;
    QVector<quint8> m_modules;
    int m_width = 0;
    QColor m_dark = Qt::black;
    QColor m_light = Qt::white;
};
