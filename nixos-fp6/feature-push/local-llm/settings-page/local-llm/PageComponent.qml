import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Fairphone.LocalLlm 1.0

ItemPage {
    title: "On-device AI models"
    flickable: models
    ModelsView { id: models; anchors.fill: parent }
}
