// FP6 quick settings: tile grid layout (row-major, wide tiles span 2 columns,
// 3 with 6 columns; a wide tile that does not fit at the end of a row starts
// the next row).
.pragma library

function wideSpan(columns) { return columns >= 6 ? 3 : 2; }
// small-tile labels get two lines when the cells are narrow
function labelLines(columns) { return columns >= 5 ? 2 : 1; }

function layout(ids, wideIds, columns, cellW, cellH) {
    var pos = {}, row = 0, col = 0;
    for (var i = 0; i < ids.length; i++) {
        var id = ids[i];
        var span = wideIds.indexOf(id) >= 0 ? wideSpan(columns) : 1;
        if (col + span > columns) { row++; col = 0; }
        pos[id] = { x: col * cellW, y: row * cellH, w: span * cellW, h: cellH, row: row, span: span, index: i };
        col += span;
        if (col >= columns) { row++; col = 0; }
    }
    return { pos: pos, rows: col === 0 ? row : row + 1 };
}

// Index in `ids` (which no longer contains the dragged tile) where a tile
// dropped at (px, py) goes.
function dropIndex(ids, pos, px, py) {
    for (var i = 0; i < ids.length; i++) {
        var p = pos[ids[i]];
        if (!p) continue;
        if (py < p.y) return i;
        if (py < p.y + p.h && px < p.x + p.w / 2) return i;
    }
    return ids.length;
}

function without(list, id) {
    var out = [];
    for (var i = 0; i < list.length; i++) if (list[i] !== id) out.push(list[i]);
    return out;
}

function inserted(list, id, index) {
    var out = without(list, id);
    out.splice(Math.max(0, Math.min(index, out.length)), 0, id);
    return out;
}

function sameList(a, b) {
    if (a.length !== b.length) return false;
    for (var i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
    return true;
}

// gsettings 'as' values arrive as JS arrays or QVariantList; normalise.
function toList(v) {
    var out = [];
    if (!v) return out;
    for (var i = 0; i < v.length; i++) out.push(String(v[i]));
    return out;
}
