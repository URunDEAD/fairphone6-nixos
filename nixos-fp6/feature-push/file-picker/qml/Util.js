.pragma library

// file:// URL for a local path (each segment percent-encoded, so '#', '?'
// and '%' in names survive).
function fileUrl(path) {
    return "file://" + path.split("/").map(encodeURIComponent).join("/");
}

function isInside(path, dir) {
    if (!path || !dir)
        return false;
    if (dir === "/")
        return true;
    return path === dir || path.indexOf(dir + "/") === 0;
}

function prettyPath(path, home) {
    if (!path)
        return "";
    if (home && home !== "/" && isInside(path, home))
        return "~" + path.substring(home.length);
    return path;
}

function fmtSize(b) {
    if (b === undefined || b < 0)
        return "";
    if (b < 1024)
        return b + " B";
    var u = ["KB", "MB", "GB", "TB"];
    var v = b / 1024, i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return (v < 10 ? v.toFixed(1) : Math.round(v)) + " " + u[i];
}

function fmtDate(d) {
    if (!d || isNaN(d.getTime()))
        return "";
    var now = new Date();
    if (d.toDateString() === now.toDateString())
        return Qt.formatTime(d, Qt.locale().timeFormat(1 /* Locale.ShortFormat */));
    return Qt.formatDate(d, Qt.locale().dateFormat(1 /* Locale.ShortFormat */));
}

var groups = [
    [/\.(png|jpe?g|gif|webp|bmp|svgz?|heic|heif|avif|tiff?|dng|raw)$/i, "image-x-generic-symbolic"],
    [/\.(mp3|ogg|oga|opus|flac|wav|m4a|aac|wma|mid|midi)$/i, "audio-x-generic-symbolic"],
    [/\.(mp4|mkv|webm|avi|mov|m4v|3gp|ogv|wmv|mpe?g)$/i, "video-x-generic-symbolic"],
    [/\.pdf$/i, "application-pdf-symbolic"],
    [/\.(zip|tar|gz|tgz|bz2|xz|zst|7z|rar|torrent|deb|rpm|apk|click)$/i, "package-x-generic-symbolic"],
    [/\.(odt|docx?|rtf|epub)$/i, "x-office-document-symbolic"],
    [/\.(ods|xlsx?|csv)$/i, "x-office-spreadsheet-symbolic"],
    [/\.(odp|pptx?)$/i, "x-office-presentation-symbolic"],
    [/\.(html?|xhtml)$/i, "text-html-symbolic"],
    [/\.(xml|json|ya?ml|toml|nix)$/i, "text-xml-symbolic"],
];

function iconFor(name) {
    for (var i = 0; i < groups.length; i++)
        if (groups[i][0].test(name))
            return groups[i][1];
    return "text-x-generic-symbolic";
}

function optionLabel(options, id) {
    for (var i = 0; i < options.length; i++)
        if (options[i].id === id)
            return options[i].label;
    return options.length > 0 ? options[0].label : "";
}
