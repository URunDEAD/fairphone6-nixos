// Preview stub of Lomiri.Telephony's ContactWatcher (names from FakeContacts).
import QtQuick 2.15
QtObject {
    property string identifier
    property var addressableFields: []
    readonly property string alias: FakeContacts.names[identifier] || ""
    readonly property string contactId: alias !== "" ? "fake-" + identifier : ""
    readonly property bool isUnknown: alias === ""
    readonly property string avatar: ""
    readonly property bool interactive: true
    property var details: ({})
}
