import QtQuick 2.12
import Lomiri.Content 1.3
Item {
    id: content
    signal received(var items, string mime, string source)
    signal finished(string message)
    property var incoming: null
    property var retainedImports: []
    property var outgoing: null
    property var consumed: null
    property var heldItems: []
    ContentStore { id: store; scope: ContentScope.App }
    ContentPeer { id: destination; handler: ContentHandler.Share; selectionType: ContentTransfer.Multiple }
    Component { id: itemFactory; ContentItem {} }
    function typeFor(name) {
        return name === "contacts" ? ContentType.Contacts : name === "events" ? ContentType.Events : name === "ebooks" ? ContentType.EBooks : name === "pictures" ? ContentType.Pictures : name === "music" ? ContentType.Music : name === "videos" ? ContentType.Videos : name === "links" ? ContentType.Links : name === "text" ? ContentType.Text : ContentType.Documents
    }
    function receive(transfer) {
        if (retainedImports.length >= 32) { transfer.state = ContentTransfer.Aborted; finished("Too many incoming shares. Close the sheet and retry."); return }
        retainedImports.push(transfer); incoming = transfer; collect()
    }
    function collect() {
        if (!incoming || incoming === consumed || incoming.state !== ContentTransfer.Charged || !incoming.items.length) return
        consumed = incoming
        var items = []
        var kind = incoming.contentType
        var mime = kind === ContentType.Pictures ? "image/*" : kind === ContentType.Music ? "audio/*" : kind === ContentType.Videos ? "video/*" : kind === ContentType.Contacts ? "text/vcard" : kind === ContentType.Events ? "text/calendar" : kind === ContentType.EBooks ? "application/epub+zip" : kind === ContentType.Text ? "text/plain" : "application/octet-stream"
        for (var i = 0; i < incoming.items.length; ++i) {
            var item = incoming.items[i]
            var url = item.url.toString()
            if (url.indexOf("file:") === 0) {
                items.push(url)
                if (/\.(png|jpe?g|webp)$/i.test(url)) mime = /\.png$/i.test(url) ? "image/png" : /\.webp$/i.test(url) ? "image/webp" : "image/jpeg"
                else if (/\.pdf$/i.test(url)) mime = "application/pdf"
            } else { items.push(item.text.length ? item.text : url); mime = "text/plain" }
        }
        received(items, mime, incoming.source)
        // Retain incoming transfer so Content Hub keeps temporary files available.
    }
    function send(appId, request, kind) {
        if (outgoing && outgoing.state !== ContentTransfer.Collected && outgoing.state !== ContentTransfer.Aborted) { finished("Wait for the receiving app to collect the previous share."); return }
        destination.appId = appId
        destination.contentType = typeFor(kind)
        outgoing = destination.request(store)
        if (!outgoing) { finished("Could not open the receiving app."); return }
        var values = []
        for (var i = 0; i < request.items.length; ++i) {
            var value = request.items[i]
            values.push(itemFactory.createObject(content, {url: value.indexOf("file:") === 0 || kind === "links" ? value : "", text: value.indexOf("file:") === 0 ? "" : value}))
        }
        heldItems = values
        outgoing.items = values
        outgoing.state = ContentTransfer.Charged
        finished("Shared with the app. Keep this window open until it collects the items.")
    }
    Connections { target: ContentHub; function onShareRequested(transfer) { content.receive(transfer) } function onImportRequested(transfer) { content.receive(transfer) } }
    Connections { target: content.incoming; function onStateChanged() { content.collect() } function onItemsChanged() { content.collect() } }
    Connections {
        target: content.outgoing
        function onStateChanged() {
            if (content.outgoing.state === ContentTransfer.Collected) content.finished("The app collected the shared items.")
            else if (content.outgoing.state === ContentTransfer.Aborted) content.finished("The app canceled this share.")
        }
    }
}
