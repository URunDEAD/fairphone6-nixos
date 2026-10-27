// SPDX-License-Identifier: GPL-3.0-only
//
// FP6.Accessibility ColourMatrix: turns the accessibility colour settings
// (inversion, correction type and strength) into the 3x4 affine matrix that
// FP6.DisplayComfort ColorFilter.preMatrix takes. `matrix` is empty while
// nothing is on, so the colour filter keeps drawing nothing (or only the
// night light blend).
#pragma once

#include <QtCore/QObject>
#include <QtCore/QVariantList>
#include <QtGui/QColor>

class ColourMatrix : public QObject
{
    Q_OBJECT
    Q_PROPERTY(bool inversion READ inversion WRITE setInversion NOTIFY inversionChanged)
    // "", "protanomaly", "deuteranomaly", "tritanomaly", "greyscale"
    Q_PROPERTY(QString correction READ correction WRITE setCorrection NOTIFY correctionChanged)
    // 0..1 (1 = Android's full correction)
    Q_PROPERTY(qreal strength READ strength WRITE setStrength NOTIFY strengthChanged)
    Q_PROPERTY(QVariantList matrix READ matrix NOTIFY matrixChanged)
    Q_PROPERTY(bool active READ active NOTIFY matrixChanged)

public:
    explicit ColourMatrix(QObject *parent = nullptr);

    bool inversion() const { return m_inversion; }
    QString correction() const { return m_correction; }
    qreal strength() const { return m_strength; }
    QVariantList matrix() const { return m_matrix; }
    bool active() const { return !m_matrix.isEmpty(); }

    void setInversion(bool v);
    void setCorrection(const QString &v);
    void setStrength(qreal v);

    // The colour a viewer with `deficiency` sees for `colour` after this
    // matrix (Settings preview helper; "" = normal vision).
    Q_INVOKABLE QColor preview(const QColor &colour, const QString &deficiency = QString()) const;
    // simulation(deficiency) * matrix (as a preMatrix): the whole screen as
    // that viewer sees it with this correction (previews and tests).
    Q_INVOKABLE QVariantList simulated(const QString &deficiency) const;

Q_SIGNALS:
    void inversionChanged();
    void correctionChanged();
    void strengthChanged();
    void matrixChanged();

private:
    void recompute();
    bool m_inversion = false;
    QString m_correction;
    qreal m_strength = 1;
    QVariantList m_matrix;
};
