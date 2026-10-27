import QtQuick 2.12
import Lomiri.Components 1.3

// Synthetic, offline prototype. No D-Bus access or lock-screen bypass.
IncomingCall {
    id: surface
    width: 400
    height: 800
    property var args: Qt.application.arguments
    property string capturePath: {
        for (var i = 0; i < args.length; i++) if (args[i].indexOf("--capture=") === 0) return args[i].substring(10)
        return ""
    }
    property string scenario: args.indexOf("--active") >= 0 ? "active" : (args.indexOf("--held") >= 0 ? "held" : "ringing")
    callState: scenario
    silent: args.indexOf("--silent") >= 0
    cellularBusy: args.indexOf("--waiting") >= 0
    providerName: "Synthetic VoIP · Preview"
    onActionRequested: {
        if (action === "answer" || action === "resume") callState = "active"
        else if (action === "hold") callState = "held"
        else if (action === "end") callState = "ended"
        else if (action === "mute" || action === "unmute") muted = !muted
    }
    onRouteRequested: surface.route = route
    Timer {
        interval: 900; running: surface.capturePath !== ""; repeat: false
        onTriggered: {
            
            surface.grabToImage(function(result) {
                if (!result.saveToFile(surface.capturePath)) {
                    console.error("Could not save preview")
                    Qt.exit(1)
                } else Qt.quit()
            })
        }
    }
}
