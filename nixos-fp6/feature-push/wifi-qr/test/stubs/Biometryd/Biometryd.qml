// Preview stub of biometryd's QML module: a reader that never answers.
pragma Singleton
import QtQuick 2.12
QtObject {
    property bool available: true
    property QtObject defaultDevice: QtObject {
        property QtObject identifier: QtObject {
            function identifyUser() {
                return { start: function (observer) {}, cancel: function () {} };
            }
        }
    }
}
