import QtQuick 2.12
import QtTest 1.2
import Lomiri.Components 1.3
import "../../qml"
MainView {
    applicationName: "fp6-bluetooth-audio-test"
    width: 450; height: 900
    QtObject {
        id: backend
        property int writes: 0
        property string selected: ""
        property bool busy: false
        property bool available: true
        property string error: ""
        property var devices: [{address:"00:00:00:00:00:01",id:"test",profile:"a2dp-sink",codec:"ldac",bitrate:null,profiles:[
            {id:"a2dp-sink",codec:"ldac",kind:"A2DP",available:true},
            {id:"a2dp-sink-sbc",codec:"sbc",kind:"A2DP",available:true},
            {id:"a2dp-sink-aac",codec:"aac",kind:"A2DP",available:false}]}]
        function selectProfile(d,p) { writes++; selected=p }
        function prefer(d,q) { writes++; selected=q ? "quality" : "stable" }
    }
    Fp6AudioQuality { id: quality; width: parent.width; audio: backend; address:"00:00:00:00:00:01"; connected:true }
    TestCase {
        name: "AudioQuality"
        when: windowShown
        function test_01_readonlyBinding() {
            wait(100)
            compare(backend.writes,0)
            compare(quality.profiles.length,2)
            var picker=findChild(quality,"audioCodecPicker")
            compare(picker.selectedIndex,0)
            compare(findChild(quality,"preferAudioQuality").checked,true)
        }
        function test_02_explicitSelection() {
            findChild(quality,"audioCodecPicker").delegateClicked(1)
            compare(backend.writes,1)
            compare(backend.selected,"a2dp-sink-sbc")
        }
        function test_03_statusUpdateDoesNotWrite() {
            backend.devices=[]
            wait(20)
            compare(quality.device,null)
            compare(backend.writes,1)
        }
        function test_04_hiddenWithoutHelper() {
            compare(quality.visible,true)
            backend.available=false
            compare(quality.visible,false)
            backend.available=true
        }
    }
}
