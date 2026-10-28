// An e-mail account (IMAP + SMTP). The password (or app password) goes to
// the keyring.
import QtQuick 2.12
import Lomiri.Components 1.3

Page {
    id: page
    property var account: null
    property bool hasKey: false
    property var settingsPage: null
    readonly property bool isNew: account === null

    function save() {
        var id = isNew ? (addr.text.split("@")[0].replace(/[^a-zA-Z0-9]/g, "").toLowerCase() || "mail") + "-" + (Date.now() % 100000) : account.id
        var a = {
            id: id, name: name.text || addr.text, address: addr.text.trim(),
            imap_host: imap.text.trim(), imap_port: parseInt(imapPort.text) || 993,
            smtp_host: smtp.text.trim(), smtp_port: parseInt(smtpPort.text) || 465,
            username: user.text.trim(), smtp_security: sec.selectedIndex === 0 ? "tls" : "starttls"
        }
        settingsPage.save(function(c) {
            var f = false
            for (var i = 0; i < c.email_accounts.length; i++) if (c.email_accounts[i].id === id) { c.email_accounts[i] = a; f = true }
            if (!f) c.email_accounts.push(a)
        })
        if (pass.text !== "") { pw.key = "email:" + id; pw.value = pass.text; pass.text = ""; pw.start() }
    }
    Timer { id: pw; interval: 400; property string key; property string value
            onTriggered: root.call("SetKey", [key, value], function(r, err) { value = ""; if (err) root.showError(err) }) }

    header: PageHeader {
        id: header
        title: isNew ? i18n.tr("Add an e-mail account") : account.name
        trailingActionBar.actions: [Action { iconName: "ok"; text: i18n.tr("Save"); onTriggered: { page.save(); stack.pop() } }]
    }
    Flickable {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        contentHeight: col.height + units.gu(4)
        clip: true
        Column {
            id: col
            x: units.gu(2); width: parent.width - units.gu(4)
            spacing: units.gu(1)
            Item { width: 1; height: units.gu(1) }
            Label { text: i18n.tr("Name") }
            TextField { id: name; width: parent.width; text: account ? account.name : ""; placeholderText: i18n.tr("Work") }
            Label { text: i18n.tr("Address") }
            TextField { id: addr; width: parent.width; text: account ? account.address : ""; placeholderText: "test@example.com"
                        inputMethodHints: Qt.ImhEmailCharactersOnly | Qt.ImhNoPredictiveText
                        onTextChanged: if (page.isNew && addr.text.indexOf("@") > 0) {
                            var d = addr.text.split("@")[1]
                            if (imap.text === "" || imap.text.indexOf("imap.") === 0) imap.text = "imap." + d
                            if (smtp.text === "" || smtp.text.indexOf("smtp.") === 0) smtp.text = "smtp." + d
                        } }
            Label { text: i18n.tr("Incoming (IMAP, TLS)") }
            Row { width: parent.width; spacing: units.gu(1)
                TextField { id: imap; width: parent.width - imapPort.width - units.gu(1); text: account ? account.imap_host : ""; inputMethodHints: Qt.ImhUrlCharactersOnly }
                TextField { id: imapPort; width: units.gu(9); text: account ? String(account.imap_port) : "993"; inputMethodHints: Qt.ImhDigitsOnly } }
            Label { text: i18n.tr("Outgoing (SMTP)") }
            Row { width: parent.width; spacing: units.gu(1)
                TextField { id: smtp; width: parent.width - smtpPort.width - units.gu(1); text: account ? account.smtp_host : ""; inputMethodHints: Qt.ImhUrlCharactersOnly }
                TextField { id: smtpPort; width: units.gu(9); text: account ? String(account.smtp_port) : "465"; inputMethodHints: Qt.ImhDigitsOnly } }
            OptionSelector { id: sec; text: i18n.tr("SMTP security"); model: [i18n.tr("TLS (port 465)"), i18n.tr("STARTTLS (port 587)")]
                             selectedIndex: account && account.smtp_security === "starttls" ? 1 : 0
                             onSelectedIndexChanged: smtpPort.text = selectedIndex === 0 ? "465" : "587" }
            Label { text: i18n.tr("User name (if not the address)") }
            TextField { id: user; width: parent.width; text: account ? account.username : ""; inputMethodHints: Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase }
            Label { text: page.hasKey ? i18n.tr("Password (saved; type to replace)") : i18n.tr("Password or app password") }
            TextField { id: pass; width: parent.width; echoMode: TextInput.Password; inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText }
            Button { text: i18n.tr("Delete this account"); visible: !page.isNew; color: theme.palette.normal.negative
                     onClicked: { root.call("SetKey", ["email:" + account.id, ""])
                                  settingsPage.save(function(c) { c.email_accounts = c.email_accounts.filter(function(a) { return a.id !== account.id }) })
                                  stack.pop() } }
        }
    }
}
