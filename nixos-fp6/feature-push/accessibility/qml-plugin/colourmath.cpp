// SPDX-License-Identifier: GPL-3.0-only
#include "colourmath.h"

#include <cmath>
#include <cstring>

namespace fp6a11y {
namespace {

// 3x3 helpers (row-major).
void mul3(const double a[9], const double b[9], double out[9])
{
    for (int r = 0; r < 3; ++r)
        for (int c = 0; c < 3; ++c) {
            double s = 0;
            for (int k = 0; k < 3; ++k) s += a[r * 3 + k] * b[k * 3 + c];
            out[r * 3 + c] = s;
        }
}

bool inv3(const double m[9], double out[9])
{
    const double a = m[0], b = m[1], c = m[2], d = m[3], e = m[4], f = m[5], g = m[6], h = m[7], i = m[8];
    const double A = e * i - f * h, B = -(d * i - f * g), C = d * h - e * g;
    const double det = a * A + b * B + c * C;
    if (std::fabs(det) < 1e-12) return false;
    const double D = -(b * i - c * h), E = a * i - c * g, F = -(a * h - b * g);
    const double G = b * f - c * e, H = -(a * f - c * d), I = a * e - b * d;
    const double r[9] = { A, D, G, B, E, H, C, F, I };
    for (int k = 0; k < 9; ++k) out[k] = r[k] / det;
    return true;
}

void cross(const double a[3], const double b[3], double out[3])
{
    out[0] = a[1] * b[2] - a[2] * b[1];
    out[1] = a[2] * b[0] - a[0] * b[2];
    out[2] = a[0] * b[1] - a[1] * b[0];
}

void to12(const double m[9], float out[12])
{
    for (int r = 0; r < 3; ++r) {
        for (int c = 0; c < 3; ++c) out[r * 4 + c] = float(m[r * 3 + c]);
        out[r * 4 + 3] = 0;
    }
}

// linear RGB (sRGB primaries) -> XYZ, XYZ -> LMS (CIECAM02 CAT02), as in
// SurfaceFlinger's Daltonizer.cpp (there written column-major).
const double kRgb2Xyz[9] = { 0.4124, 0.3576, 0.1805,
                             0.2126, 0.7152, 0.0722,
                             0.0193, 0.1192, 0.9505 };
const double kXyz2Lms[9] = { 0.7328, 0.4296, -0.1624,
                            -0.7036, 1.6975, 0.0061,
                             0.0030, 0.0136, 0.9834 };

struct Lms {
    double rgb2lms[9], lms2rgb[9];
    double p0[3], p1[3];   // planes for protan/deutan, tritan
    Lms()
    {
        mul3(kXyz2Lms, kRgb2Xyz, rgb2lms);
        inv3(rgb2lms, lms2rgb);
        const double red[3] = { rgb2lms[0], rgb2lms[3], rgb2lms[6] };
        const double blue[3] = { rgb2lms[2], rgb2lms[5], rgb2lms[8] };
        const double white[3] = { rgb2lms[0] + rgb2lms[1] + rgb2lms[2],
                                  rgb2lms[3] + rgb2lms[4] + rgb2lms[5],
                                  rgb2lms[6] + rgb2lms[7] + rgb2lms[8] };
        cross(white, blue, p0);
        cross(white, red, p1);
    }
};

void simulationLms(Correction type, const Lms &k, double s[9])
{
    const double id[9] = { 1, 0, 0, 0, 1, 0, 0, 0, 1 };
    std::memcpy(s, id, sizeof id);
    switch (type) {
    case Correction::Protanomaly:   // L from M, S
        s[0] = 0; s[1] = -k.p0[1] / k.p0[0]; s[2] = -k.p0[2] / k.p0[0];
        break;
    case Correction::Deuteranomaly: // M from L, S
        s[3] = -k.p0[0] / k.p0[1]; s[4] = 0; s[5] = -k.p0[2] / k.p0[1];
        break;
    case Correction::Tritanomaly:   // S from L, M
        s[6] = -k.p1[0] / k.p1[2]; s[7] = -k.p1[1] / k.p1[2]; s[8] = 0;
        break;
    default:
        break;
    }
}

void errorSpread(Correction type, double e[9])
{
    const double z[9] = { 0, 0, 0, 0, 0, 0, 0, 0, 0 };
    std::memcpy(e, z, sizeof z);
    // SurfaceFlinger's errp/errd/errt (the 1.0 diagonal included): the
    // lost cone's error is added back to itself (it is projected away again
    // by the eye) and 0.7 of it to the two other cones.
    e[0] = e[4] = e[8] = 1.0;
    switch (type) {
    case Correction::Protanomaly:   e[3] = 0.7; e[6] = 0.7; break;
    case Correction::Deuteranomaly: e[1] = 0.7; e[7] = 0.7; break;
    case Correction::Tritanomaly:   e[2] = 0.7; e[5] = 0.7; break;
    default: break;
    }
}

} // namespace

Correction correctionFromString(const char *s)
{
    if (!s) return Correction::None;
    if (!std::strcmp(s, "protanomaly")) return Correction::Protanomaly;
    if (!std::strcmp(s, "deuteranomaly")) return Correction::Deuteranomaly;
    if (!std::strcmp(s, "tritanomaly")) return Correction::Tritanomaly;
    if (!std::strcmp(s, "greyscale") || !std::strcmp(s, "grayscale")) return Correction::Greyscale;
    return Correction::None;
}

void identity(float out[12])
{
    static const float id[12] = { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0 };
    std::memcpy(out, id, sizeof id);
}

bool isIdentity(const float m[12], float eps)
{
    float id[12];
    identity(id);
    for (int i = 0; i < 12; ++i)
        if (std::fabs(m[i] - id[i]) > eps) return false;
    return true;
}

void compose(const float a[12], const float b[12], float out[12])
{
    // [A|s][B|t] = [AB | At + s]
    for (int r = 0; r < 3; ++r) {
        for (int c = 0; c < 4; ++c) {
            float sum = c == 3 ? a[r * 4 + 3] : 0.f;
            for (int k = 0; k < 3; ++k) sum += a[r * 4 + k] * b[k * 4 + c];
            out[r * 4 + c] = sum;
        }
    }
}

void apply(const float m[12], const float rgb[3], float out[3])
{
    for (int r = 0; r < 3; ++r)
        out[r] = m[r * 4] * rgb[0] + m[r * 4 + 1] * rgb[1] + m[r * 4 + 2] * rgb[2] + m[r * 4 + 3];
}

void scaleTowardsIdentity(float m[12], float k)
{
    float id[12];
    identity(id);
    if (k < 0) k = 0;
    if (k > 1) k = 1;
    for (int i = 0; i < 12; ++i) m[i] = id[i] + k * (m[i] - id[i]);
}

void inversion(float out[12])
{
    static const double Y[3] = { 0.299, 0.587, 0.114 };
    for (int r = 0; r < 3; ++r) {
        for (int c = 0; c < 3; ++c) out[r * 4 + c] = float((r == c ? 1.0 : 0.0) - 2.0 * Y[c]);
        out[r * 4 + 3] = 1;
    }
}

void plainInversion(float out[12])
{
    static const float m[12] = { -1, 0, 0, 1, 0, -1, 0, 1, 0, 0, -1, 1 };
    std::memcpy(out, m, sizeof m);
}

void greyscale(float out[12])
{
    static const float L[3] = { 0.2126f, 0.7152f, 0.0722f };
    for (int r = 0; r < 3; ++r) {
        for (int c = 0; c < 3; ++c) out[r * 4 + c] = L[c];
        out[r * 4 + 3] = 0;
    }
}

void simulate(Correction type, float out[12])
{
    if (type == Correction::None || type == Correction::Greyscale) {
        if (type == Correction::Greyscale) greyscale(out); else identity(out);
        return;
    }
    static const Lms k;
    double s[9], t[9], m[9];
    simulationLms(type, k, s);
    mul3(s, k.rgb2lms, t);
    mul3(k.lms2rgb, t, m);
    to12(m, out);
}

void daltonize(Correction type, float out[12])
{
    if (type == Correction::None) { identity(out); return; }
    if (type == Correction::Greyscale) { greyscale(out); return; }
    static const Lms k;
    // lms2rgb * (S * rgb2lms + E * (rgb2lms - S * rgb2lms))
    double s[9], e[9], srgb[9], diff[9], espread[9], sum[9], m[9];
    simulationLms(type, k, s);
    errorSpread(type, e);
    mul3(s, k.rgb2lms, srgb);
    for (int i = 0; i < 9; ++i) diff[i] = k.rgb2lms[i] - srgb[i];
    mul3(e, diff, espread);
    for (int i = 0; i < 9; ++i) sum[i] = srgb[i] + espread[i];
    mul3(k.lms2rgb, sum, m);
    to12(m, out);
}

void accessibilityMatrix(bool invert, Correction type, float strength, float out[12])
{
    float corr[12];
    daltonize(type, corr);
    scaleTowardsIdentity(corr, strength);
    if (!invert) {
        std::memcpy(out, corr, sizeof corr);
        return;
    }
    float inv[12];
    inversion(inv);
    compose(inv, corr, out);
}

} // namespace fp6a11y
