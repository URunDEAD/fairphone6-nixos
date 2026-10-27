// Preview stub of Lomiri.Contacts' ContactAvatar.
import QtQuick 2.15
import Lomiri.Components 1.3
LomiriShape {
    property string fallbackAvatarUrl
    property string fallbackDisplayName
    property bool showAvatarPicture
    property string initials: fallbackDisplayName.length > 0 ? fallbackDisplayName[0] : ""
    aspect: LomiriShape.Flat
    backgroundColor: theme.palette.normal.base
}
