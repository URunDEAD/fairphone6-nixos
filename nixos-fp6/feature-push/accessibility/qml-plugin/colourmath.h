// SPDX-License-Identifier: GPL-3.0-only
//
// Colour maths of the FP6 accessibility colour filters. Every transform is a
// row-major 3x4 affine matrix on gamma-encoded sRGB in 0..1
// (r' = m[0] r + m[1] g + m[2] b + m[3], g' = m[4..7], b' = m[8..11]), the
// format of FP6.DisplayComfort ColorFilter.preMatrix. The shell applies
//   night light * greyscale * inversion * correction
// (correction first, night light last: the order of Android's
// DisplayTransformManager levels and SurfaceFlinger's daltonizer).
#pragma once

namespace fp6a11y {

enum class Correction { None, Protanomaly, Deuteranomaly, Tritanomaly, Greyscale };

// Parses "protanomaly", "deuteranomaly", "tritanomaly", "greyscale"
// (also "grayscale"); anything else is None.
Correction correctionFromString(const char *s);

void identity(float out[12]);
bool isIdentity(const float m[12], float eps = 1e-4f);
// out = a * b (b applied first). out may alias neither input.
void compose(const float a[12], const float b[12], float out[12]);
// Applies m to rgb (no clamping).
void apply(const float m[12], const float rgb[3], float out[3]);
// m = I + k (m - I): strength k in 0..1.
void scaleTowardsIdentity(float m[12], float k);

// Android's colour inversion (DisplayTransformManager MATRIX_INVERT_COLOR):
// invert, then rotate the hue by 180 degrees around the Rec. 601 grey axis,
// i.e. x' = 1 + x - 2 Y601(x): black <-> white, hues keep their colour.
void inversion(float out[12]);
// Plain RGB inversion x' = 1 - x (for comparison / tests).
void plainInversion(float out[12]);
// Rec. 709 luma greyscale.
void greyscale(float out[12]);
// SurfaceFlinger's Daltonizer in correction mode: simulate the missing cone
// (projection in LMS onto the plane through black, white and blue (L/M) or
// red (S)), then spread the error onto the remaining cones. Greys stay grey.
void daltonize(Correction type, float out[12]);
// Same projection without the error spread: what the viewer with the
// deficiency sees (simulation mode, used by tests and the Settings preview).
void simulate(Correction type, float out[12]);

// The accessibility matrix: inversion (optional) * correction (strength
// 0..1, None = identity).
void accessibilityMatrix(bool invert, Correction type, float strength, float out[12]);

} // namespace fp6a11y
