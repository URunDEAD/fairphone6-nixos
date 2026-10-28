// UITK font scale check (offscreen, software Qt Quick): prints the pixel
// sizes UITK computes and saves a grab of the labels.
// Writes grab.png into the working directory.
import QtQuick 2.12
import Lomiri.Components 1.3

Rectangle {
    id: root
    width: 900
    height: 700
    color: "white"

    Column {
        id: column
        spacing: 0
        Label { id: xsmall; text: "Hamburgefonstiv 123"; textSize: Label.XSmall; color: "black" }
        Label { id: small; text: "Hamburgefonstiv 123"; textSize: Label.Small; color: "black" }
        Label { id: medium; text: "Hamburgefonstiv 123"; color: "black" }
        Label { id: large; text: "Hamburgefonstiv 123"; textSize: Label.Large; color: "black" }
        Label { id: xlarge; text: "Hamburgefonstiv 123"; textSize: Label.XLarge; color: "black" }
        Text { id: fu; text: "Hamburgefonstiv 123"; font.pixelSize: FontUtils.sizeToPixels("large"); color: "black" }
    }

    Timer {
        interval: 300
        running: true
        onTriggered: {
            console.log("UITK-FONT-RESULT " + JSON.stringify({
                gu: units.gu(1),
                xsmall: xsmall.font.pixelSize, small: small.font.pixelSize,
                medium: medium.font.pixelSize, large: large.font.pixelSize,
                xlarge: xlarge.font.pixelSize, fontUtilsLarge: fu.font.pixelSize,
                mediumHeight: medium.contentHeight, mediumWeight: medium.font.weight,
                appWeight: Qt.application.font.weight, columnHeight: column.height,
                columnWidth: column.width
            }));
            column.grabToImage(function (result) {
                result.saveToFile("grab.png");
                Qt.quit();
            });
        }
    }
}
