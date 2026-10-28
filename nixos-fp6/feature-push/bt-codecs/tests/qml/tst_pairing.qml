import QtQuick 2.12
import QtTest 1.2
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
MainView {
    id: root
    width: 450; height: 900
    applicationName: "fp6-pairing-test"
    TestCase {
        name: "PairingInput"
        when: windowShown
        function checkDialog(name, valid, invalid) {
            var component=Qt.createComponent("generated/" + name)
            compare(component.status,Component.Ready,component.errorString())
            var dialog=PopupUtils.open(component,root,{name:"Test Headphones"})
            wait(50)
            var input=findChild(dialog,"pairingInput")
            var submit=findChild(dialog,"pairingSubmit")
            verify(input !== null)
            compare(input.text,"")
            compare(submit.enabled,false)
            input.text=valid
            compare(submit.enabled,true)
            input.text=invalid
            compare(submit.enabled,false)
            PopupUtils.close(dialog)
            wait(50)
        }
        function test_passkeyValidation() { checkDialog("ProvidePasskeyDialog.qml","000001","abc") }
        function test_pinValidation() { checkDialog("ProvidePinCodeDialog.qml","12ab","") }
    }
}
