/*
 * FP6 charging feedback: a level ring (one shader pass) with rounded caps,
 * a colour gradient along the arc, a soft outer glow and an optional
 * highlight running along the arc (fast charging). A bolt in the middle.
 *
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15

Item {
    id: ring
    // 0..1 of the circle
    property real progress: 0
    property color colorA: "#3ddc84"
    property color colorB: "#a8f06a"
    property color track: Qt.rgba(1, 1, 1, 0.14)
    // ring thickness as a fraction of the diameter
    property real thickness: 0.07
    property real glow: 0.6
    // position of the running highlight (0..1 along the arc), < 0 = none
    property real sweep: -1
    property bool bolt: true
    property real boltScale: 0.36
    property color boltColor: "white"
    property real boltOpacity: 1

    ShaderEffect {
        anchors.fill: parent
        property real progress: Math.max(0, Math.min(1, ring.progress))
        property color colorA: ring.colorA
        property color colorB: ring.colorB
        property color track: ring.track
        property real thickness: ring.thickness * 2.0   // in -1..1 units
        property real glow: ring.glow
        property real sweep: ring.sweep
        property real aa: 3.0 / Math.max(8, width)
        fragmentShader: "
            #ifdef GL_ES
            precision highp float;
            #endif
            varying vec2 qt_TexCoord0;
            uniform float qt_Opacity;
            uniform float progress;
            uniform vec4 colorA;
            uniform vec4 colorB;
            uniform vec4 track;
            uniform float thickness;
            uniform float glow;
            uniform float sweep;
            uniform float aa;
            const float TAU = 6.2831853;
            void main() {
                vec2 p = qt_TexCoord0 * 2.0 - 1.0;          // y down: top is -1
                float r = length(p);
                float half_t = thickness * 0.5;
                float R = 1.0 - half_t - thickness * 0.9;     // room for the glow
                float a = atan(p.x, -p.y) / TAU;             // 0 at 12 o'clock, clockwise
                if (a < 0.0) a += 1.0;
                float d = abs(r - R);
                float band = 1.0 - smoothstep(half_t - aa, half_t + aa, d);
                // distance to the arc (0..progress) incl. round caps
                float onArc = step(a, progress) * step(0.0005, progress);
                vec2 c0 = vec2(0.0, -R);
                vec2 c1 = R * vec2(sin(TAU * progress), -cos(TAU * progress));
                float dCap = min(length(p - c0), length(p - c1));
                float dArc = mix(dCap, min(d, dCap), onArc);
                if (progress < 0.0005) dArc = 10.0;
                float fill = 1.0 - smoothstep(half_t - aa, half_t + aa, dArc);
                float t = clamp(a / max(progress, 0.001), 0.0, 1.0);
                // the caps take the colour of their own end
                if (onArc < 0.5) t = length(p - c0) < length(p - c1) ? 0.0 : 1.0;
                vec3 col = mix(colorA.rgb, colorB.rgb, t);
                // running highlight
                float hl = 0.0;
                if (sweep >= 0.0) {
                    float da = abs(a - sweep * progress);
                    da = min(da, 1.0 - da);
                    hl = exp(-da * da / 0.0012) * onArc;
                }
                col = mix(col, vec3(1.0), 0.55 * hl);
                // soft glow, faded out before the item's edge (no visible box)
                float g = glow * exp(-max(dArc - half_t, 0.0) / (thickness * 0.4)) * (1.0 - fill) * step(0.0005, progress)
                          * (1.0 - smoothstep(R + half_t, 1.0, r)) * smoothstep(0.0, R - half_t, r);
                vec4 trackCol = vec4(track.rgb * track.a, track.a) * band * (1.0 - fill);
                vec4 arcCol = vec4(col, 1.0) * fill;
                vec4 glowCol = vec4(col * g, g) * (0.55 + 0.45 * hl);
                gl_FragColor = (arcCol + trackCol * (1.0 - arcCol.a) + glowCol * (1.0 - arcCol.a)) * qt_Opacity;
            }"
    }

    Fp6ChargingBolt {
        visible: ring.bolt
        anchors.centerIn: parent
        height: parent.height * ring.boltScale
        width: height * 0.62
        color: ring.boltColor
        opacity: ring.boltOpacity
    }
}
