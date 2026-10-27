// Preview stub of Lomiri.Contacts' ContactManager (QtContacts memory store).
pragma Singleton
import QtQuick 2.15
QtObject {
    readonly property string defaultManager: "memory"
    function manager() { return "memory"; }
}
