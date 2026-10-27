// SPDX-License-Identifier: GPL-3.0-only
#include "colourmatrix.h"
#include "colourmath.h"

#include <QtGui/QColor>

ColourMatrix::ColourMatrix(QObject *parent)
    : QObject(parent)
{
}

void ColourMatrix::setInversion(bool v)
{
    if (v == m_inversion) return;
    m_inversion = v;
    Q_EMIT inversionChanged();
    recompute();
}

void ColourMatrix::setCorrection(const QString &v)
{
    if (v == m_correction) return;
    m_correction = v;
    Q_EMIT correctionChanged();
    recompute();
}

void ColourMatrix::setStrength(qreal v)
{
    v = qBound<qreal>(0, v, 1);
    if (qFuzzyCompare(v + 1, m_strength + 1)) return;
    m_strength = v;
    Q_EMIT strengthChanged();
    recompute();
}

void ColourMatrix::recompute()
{
    float m[12];
    const QByteArray c = m_correction.toLatin1();
    fp6a11y::accessibilityMatrix(m_inversion, fp6a11y::correctionFromString(c.constData()),
                                 float(m_strength), m);
    QVariantList list;
    if (!fp6a11y::isIdentity(m))
        for (float f : m) list << double(f);
    if (list == m_matrix) return;
    m_matrix = list;
    Q_EMIT matrixChanged();
}

QColor ColourMatrix::preview(const QColor &colour, const QString &deficiency) const
{
    float m[12];
    if (m_matrix.size() == 12)
        for (int i = 0; i < 12; ++i) m[i] = float(m_matrix.at(i).toDouble());
    else
        fp6a11y::identity(m);
    const float in[3] = { float(colour.redF()), float(colour.greenF()), float(colour.blueF()) };
    float out[3];
    fp6a11y::apply(m, in, out);
    for (float &f : out) f = qBound(0.f, f, 1.f);
    const QByteArray d = deficiency.toLatin1();
    const auto type = fp6a11y::correctionFromString(d.constData());
    if (type != fp6a11y::Correction::None && type != fp6a11y::Correction::Greyscale) {
        float sim[12], v[3];
        fp6a11y::simulate(type, sim);
        fp6a11y::apply(sim, out, v);
        for (int i = 0; i < 3; ++i) out[i] = qBound(0.f, v[i], 1.f);
    }
    return QColor::fromRgbF(out[0], out[1], out[2]);
}

QVariantList ColourMatrix::simulated(const QString &deficiency) const
{
    float m[12], sim[12], out[12];
    if (m_matrix.size() == 12)
        for (int i = 0; i < 12; ++i) m[i] = float(m_matrix.at(i).toDouble());
    else
        fp6a11y::identity(m);
    const QByteArray d = deficiency.toLatin1();
    fp6a11y::simulate(fp6a11y::correctionFromString(d.constData()), sim);
    fp6a11y::compose(sim, m, out);
    QVariantList list;
    for (float f : out) list << double(f);
    return list;
}
