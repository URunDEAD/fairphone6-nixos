/*
 * FP6 call recording: pick a contact's phone number for automatic recording.
 * Same contacts store as the rest of the dialer (ContactManager.manager()).
 * Installed in src/qml/SettingsPage.
 */
import QtQuick 2.9
import QtContacts 5.0
import Lomiri.Components 1.3
import Lomiri.Contacts 0.1

Page {
    id: page
    objectName: "callRecordingContactPicker"

    signal numberPicked(string number, string name)

    header: PageHeader {
        id: pageHeader
        title: i18n.tr("Choose a number")
        flickable: list
        extension: Item {
            width: parent.width
            height: units.gu(6)
            TextField {
                id: search
                objectName: "contactSearch"
                anchors { fill: parent; leftMargin: units.gu(2); rightMargin: units.gu(2); topMargin: units.gu(1); bottomMargin: units.gu(1) }
                placeholderText: i18n.tr("Search contacts")
                inputMethodHints: Qt.ImhNoPredictiveText
                primaryItem: Icon { name: "find"; width: units.gu(2); height: width }
            }
        }
    }

    ContactModel {
        id: contactModel
        manager: ContactManager.manager()
        autoUpdate: true
        sortOrders: SortOrder {
            detail: ContactDetail.DisplayLabel
            field: DisplayLabel.Label
            direction: Qt.AscendingOrder
        }
        fetchHint: FetchHint {
            detailTypesHint: [ContactDetail.DisplayLabel, ContactDetail.Name, ContactDetail.PhoneNumber]
        }
    }

    function matches(contact) {
        var q = search.text.trim().toLowerCase()
        if (q === "") return true
        var label = contact.displayLabel ? contact.displayLabel.label : ""
        if (label.toLowerCase().indexOf(q) >= 0) return true
        var nums = contact.phoneNumbers
        for (var i = 0; i < nums.length; i++)
            if (nums[i].number.indexOf(q) >= 0) return true
        return false
    }

    ListView {
        id: list
        anchors.fill: parent
        model: contactModel
        clip: true

        delegate: Column {
            id: contactDelegate
            property var contact: model.contact
            property string name: contact && contact.displayLabel ? contact.displayLabel.label : ""
            width: list.width
            visible: contact ? (contact.phoneNumbers.length > 0 && page.matches(contact)) : false
            height: visible ? implicitHeight : 0

            Repeater {
                model: contactDelegate.contact ? contactDelegate.contact.phoneNumbers : []
                delegate: ListItem {
                    height: numLayout.height + (divider.visible ? divider.height : 0)
                    onClicked: page.numberPicked(modelData.number, contactDelegate.name)
                    ListItemLayout {
                        id: numLayout
                        title.text: index === 0 ? contactDelegate.name : ""
                        subtitle.text: modelData.number
                        Icon {
                            SlotsLayout.position: SlotsLayout.Leading
                            name: "contact"
                            opacity: index === 0 ? 1 : 0
                            width: units.gu(3); height: width
                        }
                    }
                }
            }
        }
    }

    Label {
        anchors.centerIn: parent
        width: parent.width - units.gu(4)
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
        visible: list.count === 0
        color: theme.palette.normal.backgroundSecondaryText
        text: i18n.tr("No contacts with phone numbers")
    }
}
