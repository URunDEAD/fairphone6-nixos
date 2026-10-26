.pragma library

function size(b) {
    if (b === undefined || b === null || b < 0) return "";
    var u = ["B", "KB", "MB", "GB", "TB"];
    var i = 0;
    while (b >= 1000 && i < u.length - 1) { b /= 1000; i++; }
    return (i === 0 ? b.toFixed(0) : b.toFixed(b < 10 ? 2 : 1)) + " " + u[i];
}

function rate(b) { return size(b) + "/s"; }

function modeName(m) {
    switch (m) {
    case "cdrom": return "CD-ROM";
    case "disk-ro": return "USB disk (read-only)";
    case "disk-rw": return "USB disk (read-write)";
    }
    return m;
}

function kindIcon(kind) {
    switch (kind) {
    case "iso": case "iso-hybrid": return "media-optical-symbolic";
    case "disk-gpt": case "disk-mbr": case "filesystem": return "drive-harddisk-symbolic";
    case "compressed": return "stock_document";
    }
    return "drive-removable-symbolic";
}

function shortHash(h) { return h ? h.substr(0, 12) + "…" + h.substr(h.length - 8) : ""; }
