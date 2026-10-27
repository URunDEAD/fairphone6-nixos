// Back camera viewfinder, the same Qt camerabin -> fp6camerabinsrc ->
// libcamera path as Lomiri Camera and the Magnifier (the wrapper sets the
// camera stack's environment). Frames go through `filter` (QrScanFilter).
import QtQuick 2.12
import QtMultimedia 5.12

Item {
    id: view
    property bool active: false
    property var filter: null
    readonly property bool ready: camera.cameraStatus === Camera.ActiveStatus
    readonly property string error: camera.availability === Camera.Busy ? "busy"
                                    : camera.availability !== Camera.Available ? "unavailable"
                                    : camera.errorCode !== Camera.NoError ? camera.errorString : ""
    clip: true

    Camera {
        id: camera
        position: Camera.BackFace
        captureMode: Camera.CaptureViewfinder
        cameraState: view.active ? Camera.ActiveState : Camera.UnloadedState
        focus.focusMode: Camera.FocusContinuous
        focus.focusPointMode: Camera.FocusPointAuto
    }

    VideoOutput {
        id: output
        anchors.fill: parent
        source: camera
        fillMode: VideoOutput.PreserveAspectCrop
        // portrait-only app: native orientation, like Lomiri Camera
        orientation: camera.orientation
        filters: view.filter ? [view.filter] : []
    }

    // Fallback when frames cannot be read on the CPU: a grab of the output.
    function grab(callback) {
        output.grabToImage(function (result) { callback(result.image); });
    }

    function focusAt(x, y) {
        var p = output.mapPointToSourceNormalized(output.mapFromItem(view, x, y));
        camera.focus.customFocusPoint = p;
        camera.focus.focusPointMode = Camera.FocusPointCustom;
        camera.searchAndLock();
    }
}
