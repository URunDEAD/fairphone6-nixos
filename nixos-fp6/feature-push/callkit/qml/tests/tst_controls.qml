import QtQuick 2.12
import QtTest 1.2
import ".." as CallUi
Item {
    width: 400; height: 800
    CallUi.IncomingCall { id: screen; width: 400; height: 800 }
    SignalSpy { id: actions; target: screen; signalName: "actionRequested" }
    TestCase {
        name: "CallControls"
        when: windowShown
        function init() { screen.callState = "ringing"; screen.pending = false; screen.cellularBusy = false; actions.clear() }
        function test_answer_is_only_an_intent() {
            mouseClick(findChild(screen, "answerButton"))
            compare(actions.count, 1)
            compare(actions.signalArguments[0][0], "answer")
            compare(screen.callState, "ringing")
        }
        function test_cellular_disables_answer_but_allows_decline() {
            screen.cellularBusy = true
            mouseClick(findChild(screen, "answerButton"))
            compare(actions.count, 0)
            mouseClick(findChild(screen, "declineButton"))
            compare(actions.signalArguments[0][0], "end")
        }
        function test_pending_can_still_end() {
            screen.callState = "active"; screen.pending = true
            mouseClick(findChild(screen, "endButton"))
            compare(actions.signalArguments[0][0], "end")
            compare(screen.callState, "active")
        }
    }
}
