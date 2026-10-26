// Inclinometer dial: needle at `angle` (deg, 0 = up), 0-360 scale.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: dial
    property real angle: 0
    property string label: ""

    Canvas {
        anchors.fill: parent
        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            var cx = width / 2, cy = height / 2, r = width / 2 - units.gu(0.5);
            ctx.fillStyle = theme.palette.normal.foreground;
            ctx.beginPath(); ctx.arc(cx, cy, r, 0, 2 * Math.PI); ctx.fill();
            ctx.strokeStyle = theme.palette.normal.backgroundSecondaryText;
            for (var a = 0; a < 360; a += 5) {
                var t = (a - 90) * Math.PI / 180, l = a % 45 === 0 ? units.gu(2) : (a % 15 === 0 ? units.gu(1.2) : units.gu(0.6));
                ctx.lineWidth = a % 45 === 0 ? units.dp(2) : units.dp(1);
                ctx.beginPath();
                ctx.moveTo(cx + Math.cos(t) * r, cy + Math.sin(t) * r);
                ctx.lineTo(cx + Math.cos(t) * (r - l), cy + Math.sin(t) * (r - l));
                ctx.stroke();
            }
        }
    }
    Rectangle {
        width: units.dp(4); height: dial.height / 2 - units.gu(3)
        x: dial.width / 2 - width / 2
        y: units.gu(3)
        color: theme.palette.normal.activity
        transform: Rotation { origin.x: units.dp(2); origin.y: dial.height / 2 - units.gu(3); angle: dial.angle }
    }
    Label {
        anchors.centerIn: parent
        text: dial.label
        font.pixelSize: units.gu(5)
        font.weight: Font.Light
    }
}
