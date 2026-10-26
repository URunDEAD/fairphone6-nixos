// SPDX-License-Identifier: GPL-3.0-only
//
// FP6.DisplayComfort ColorFilter: a full-window colour filter for the Lomiri
// shell (night light white point + greyscale), drawn in the shell's own
// scene-graph render pass. The msm DPU cannot do this in KMS on the FP6
// panel (dual-LM/dual-DSC topology, the only DSPP sits on LM_0; Mir's
// drmModeCrtcSetGamma returns EINVAL), see display-comfort-fp6.nix.
//
//  * Identity (red = green = blue = 1, greyscale = 0): no scene-graph node at
//    all, so there is no extra draw call, no blending, no cost.
//  * Night light only (diagonal matrix): one full-window quad with a
//    multiplicative blend (dst * colour). On Adreno's tiled renderer this
//    stays in tile memory (GMEM) inside the same render pass.
//  * Greyscale (any non-diagonal matrix): the quad reads the destination with
//    GL_EXT_shader_framebuffer_fetch when the driver offers it, otherwise it
//    copies the framebuffer into a texture first (glCopyTexSubImage2D).
#pragma once

#include <QtGui/QVector3D>
#include <QtQuick/QQuickItem>

class ColorFilter : public QQuickItem
{
    Q_OBJECT
    // Channel multipliers (0..1), e.g. from a colour temperature.
    Q_PROPERTY(qreal red READ red WRITE setRed NOTIFY redChanged)
    Q_PROPERTY(qreal green READ green WRITE setGreen NOTIFY greenChanged)
    Q_PROPERTY(qreal blue READ blue WRITE setBlue NOTIFY blueChanged)
    // 0 = colour, 1 = full greyscale (Rec. 709 luma), in between = desaturated.
    Q_PROPERTY(qreal greyscale READ greyscale WRITE setGreyscale NOTIFY greyscaleChanged)
    // Colour temperature helper: sets red/green/blue for a white point in
    // Kelvin (6500 = neutral). Read back gives the last value set.
    Q_PROPERTY(int temperature READ temperature WRITE setTemperature NOTIFY temperatureChanged)
    // True while a node is drawn (anything but the identity).
    Q_PROPERTY(bool active READ active NOTIFY activeChanged)
    // "blend", "fetch", "copy" or "" (not drawn yet); for tests/diagnostics.
    Q_PROPERTY(QString path READ path NOTIFY pathChanged)

public:
    explicit ColorFilter(QQuickItem *parent = nullptr);

    qreal red() const { return m_rgb[0]; }
    qreal green() const { return m_rgb[1]; }
    qreal blue() const { return m_rgb[2]; }
    qreal greyscale() const { return m_grey; }
    int temperature() const { return m_temperature; }
    bool active() const { return m_active; }
    QString path() const { return m_path; }

    void setRed(qreal v) { setChannel(0, v); }
    void setGreen(qreal v) { setChannel(1, v); }
    void setBlue(qreal v) { setChannel(2, v); }
    void setGreyscale(qreal v);
    void setTemperature(int kelvin);

    // White point multipliers for a colour temperature, normalised so that
    // 6500 K = (1, 1, 1). Exposed for the unit test and the Settings preview.
    Q_INVOKABLE static QVector3D temperatureToRgb(int kelvin);

    // The 3x3 row-major matrix applied to (r, g, b).
    static void matrix(const float rgb[3], float grey, float out[9]);

Q_SIGNALS:
    void redChanged();
    void greenChanged();
    void blueChanged();
    void greyscaleChanged();
    void temperatureChanged();
    void activeChanged();
    void pathChanged();

protected:
    QSGNode *updatePaintNode(QSGNode *old, UpdatePaintNodeData *) override;

private:
    void setChannel(int i, qreal v);
    void changed();
    void reportPath(const QString &path);

    qreal m_rgb[3] { 1, 1, 1 };
    qreal m_grey = 0;
    int m_temperature = 6500;
    bool m_active = false;
    QString m_path;
    friend class ColorFilterNode;
};
