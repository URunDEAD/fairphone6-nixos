// SPDX-License-Identifier: GPL-3.0-only
// Unit tests of the accessibility colour maths (colourmath.h).
#include "colourmath.h"

#include <cmath>
#include <cstdio>
#include <initializer_list>

using namespace fp6a11y;

static int failures = 0;
static void check(bool ok, const char *what)
{
    std::printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok) ++failures;
}
static bool near3(const float a[3], const float b[3], float tol)
{
    for (int i = 0; i < 3; ++i)
        if (std::fabs(a[i] - b[i]) > tol) return false;
    return true;
}
static float d3(const float a[3], const float b[3])
{
    return std::sqrt((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1]) + (a[2] - b[2]) * (a[2] - b[2]));
}
static void clamp3(float v[3]) { for (int i = 0; i < 3; ++i) v[i] = v[i] < 0 ? 0 : (v[i] > 1 ? 1 : v[i]); }
static void print(const char *name, const float m[12])
{
    std::printf("INFO %s\n", name);
    for (int r = 0; r < 3; ++r)
        std::printf("INFO   % .4f % .4f % .4f | % .4f\n", m[r * 4], m[r * 4 + 1], m[r * 4 + 2], m[r * 4 + 3]);
}

int main()
{
    float m[12], v[3];
    const float black[3] = { 0, 0, 0 }, white[3] = { 1, 1, 1 }, grey[3] = { 0.25f, 0.25f, 0.25f };

    // Identity / parsing
    identity(m);
    check(isIdentity(m), "identity is identity");
    check(correctionFromString("protanomaly") == Correction::Protanomaly
          && correctionFromString("deuteranomaly") == Correction::Deuteranomaly
          && correctionFromString("tritanomaly") == Correction::Tritanomaly
          && correctionFromString("grayscale") == Correction::Greyscale
          && correctionFromString("") == Correction::None
          && correctionFromString("bogus") == Correction::None, "correction names");
    accessibilityMatrix(false, Correction::None, 1, m);
    check(isIdentity(m), "nothing on = identity (shell draws no filter)");
    accessibilityMatrix(false, Correction::Protanomaly, 0, m);
    check(isIdentity(m), "strength 0 = identity");

    // Inversion = Android's MATRIX_INVERT_COLOR (column-major there).
    inversion(m);
    print("inversion", m);
    const float android[12] = { 0.402f, -1.174f, -0.228f, 1, -0.598f, -0.174f, -0.228f, 1,
                                -0.599f, -1.175f, 0.772f, 1 };
    bool same = true;
    for (int i = 0; i < 12; ++i) same = same && std::fabs(m[i] - android[i]) < 2e-3f;
    check(same, "inversion matches Android's MATRIX_INVERT_COLOR (+-0.002)");
    apply(m, black, v); check(near3(v, white, 1e-5f), "inversion: black -> white");
    apply(m, white, v); check(near3(v, black, 1e-5f), "inversion: white -> black");
    apply(m, grey, v);
    { const float want[3] = { 0.75f, 0.75f, 0.75f }; check(near3(v, want, 1e-5f), "inversion: 25 % grey -> 75 % grey"); }
    { const float red[3] = { 1, 0, 0 }; apply(m, red, v); clamp3(v);
      check(v[0] > 0.99f && v[1] < 0.45f && v[2] < 0.45f, "inversion keeps red red (hue preserving)"); }
    { const float blue[3] = { 0, 0, 0.8f }; float p[12]; plainInversion(p); float w[3]; apply(p, blue, w);
      apply(m, blue, v); clamp3(v);
      check(v[2] > v[0] && v[2] > v[1] && w[0] > w[2], "inversion keeps blue blue; plain RGB inversion would make it yellow"); }

    // Daltonizer: greys unchanged for all types, simulation is a projection.
    const Correction types[3] = { Correction::Protanomaly, Correction::Deuteranomaly, Correction::Tritanomaly };
    const char *names[3] = { "protanomaly", "deuteranomaly", "tritanomaly" };
    for (int t = 0; t < 3; ++t) {
        char what[160];
        daltonize(types[t], m);
        print(names[t], m);
        bool g = true;
        for (float x : { 0.f, 0.2f, 0.5f, 1.f }) {
            const float in[3] = { x, x, x };
            apply(m, in, v);
            g = g && near3(v, in, 2e-3f);
        }
        std::snprintf(what, sizeof what, "%s correction keeps greys", names[t]);
        check(g, what);
        float s[12], ss[12];
        simulate(types[t], s);
        compose(s, s, ss);
        bool idem = true;
        for (int i = 0; i < 12; ++i) idem = idem && std::fabs(ss[i] - s[i]) < 1e-4f;
        std::snprintf(what, sizeof what, "%s simulation is a projection (S*S = S)", names[t]);
        check(idem, what);
        apply(s, white, v);
        std::snprintf(what, sizeof what, "%s simulation keeps white", names[t]);
        check(near3(v, white, 2e-3f), what);
        check(!isIdentity(m), "correction is not the identity");
    }

    // Correction makes confusion colours distinguishable for that viewer:
    // b = a moved along the missing cone's axis looks exactly like a to the
    // viewer (same simulation); after the correction they differ.
    for (int t = 0; t < 3; ++t) {
        float s[12], d[12];
        simulate(types[t], s);
        daltonize(types[t], d);
        const float probe[3] = { 0.9f, 0.1f, 0.5f };
        float sp[3], axis[3];
        apply(s, probe, sp);
        float n = 0;
        for (int i = 0; i < 3; ++i) { axis[i] = probe[i] - sp[i]; n += axis[i] * axis[i]; }
        n = std::sqrt(n);
        const float a[3] = { 0.5f, 0.45f, 0.4f };
        float b[3];
        for (int i = 0; i < 3; ++i) b[i] = a[i] + 0.12f * axis[i] / n;
        float sa[3], sb[3], da[3], db[3], sda[3], sdb[3];
        apply(s, a, sa); apply(s, b, sb);
        apply(d, a, da); apply(d, b, db); clamp3(da); clamp3(db);
        apply(s, da, sda); apply(s, db, sdb);
        const float before = d3(sa, sb), after = d3(sda, sdb);
        char what[200];
        std::snprintf(what, sizeof what,
                      "%s confusion pair (%.2f %.2f %.2f)/(%.2f %.2f %.2f): seen difference %.3f -> %.3f",
                      names[t], a[0], a[1], a[2], b[0], b[1], b[2], before, after);
        check(before < 1e-3f && after > 0.03f, what);   // > 25 % of the 0.12 step
    }

    // Strength scales towards identity.
    {
        float full[12], half[12];
        daltonize(Correction::Deuteranomaly, full);
        accessibilityMatrix(false, Correction::Deuteranomaly, 0.5f, half);
        bool ok = true;
        for (int i = 0; i < 12; ++i) {
            float id[12]; identity(id);
            ok = ok && std::fabs(half[i] - (id[i] + 0.5f * (full[i] - id[i]))) < 1e-6f;
        }
        check(ok, "strength 0.5 = halfway to identity");
    }

    // Composition order: inversion applied after correction.
    {
        float inv[12], corr[12], both[12], a[3], b[3];
        const float x[3] = { 0.7f, 0.2f, 0.4f };
        inversion(inv);
        daltonize(Correction::Protanomaly, corr);
        accessibilityMatrix(true, Correction::Protanomaly, 1, both);
        apply(corr, x, a); apply(inv, a, b); apply(both, x, v);
        check(near3(v, b, 1e-5f), "inversion + correction = inversion(correction(x))");
        float other[12]; compose(corr, inv, other);
        apply(other, x, a);
        check(!near3(a, v, 1e-3f), "the order matters (correction(inversion(x)) differs)");
    }

    // Greyscale mode.
    greyscale(m);
    { const float c[3] = { 1, 0, 0 }; apply(m, c, v); check(std::fabs(v[0] - 0.2126f) < 1e-5f && v[0] == v[1] && v[1] == v[2], "greyscale = Rec. 709 luma"); }
    accessibilityMatrix(true, Correction::Greyscale, 1, m);
    apply(m, white, v); check(near3(v, black, 1e-4f), "inversion + greyscale: white -> black");

    std::printf("%s (%d failures)\n", failures ? "FAILED" : "ALL PASS", failures);
    return failures ? 1 : 0;
}
