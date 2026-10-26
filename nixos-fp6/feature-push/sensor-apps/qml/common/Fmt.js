.pragma library
// Number formatting helpers shared by the apps.
function num(v, d) {
    if (v === undefined || v === null || !isFinite(v)) return "–";
    return Number(v).toFixed(d === undefined ? 1 : d);
}
function signed(v, d) {
    if (v === undefined || v === null || !isFinite(v)) return "–";
    var s = Number(v).toFixed(d === undefined ? 1 : d);
    return (v > 0 && s.charAt(0) !== "-" && Number(s) !== 0 ? "+" : "") + s;
}
function compassPoint(deg) {
    var p = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
    return p[Math.round(((deg % 360) + 360) % 360 / 22.5) % 16];
}
