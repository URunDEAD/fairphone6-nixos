// Back camera viewfinder for the Magnifier, through the same Qt camerabin ->
// fp6camerabinsrc -> libcamera path as Lomiri Camera (the launcher's
// wrapper sets the camera stack's environment). Zoom = crop of the
// viewfinder (the stream is 2048x1536, so 4x still has detail).
import QtQuick 2.12
import QtMultimedia 5.12

Item {
    id: view
    property real zoom: 1
    property bool active: false
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
        scale: view.zoom
    }

    function focusAt(x, y) {
        var p = output.mapPointToSourceNormalized(output.mapFromItem(view, x, y));
        camera.focus.customFocusPoint = p;
        camera.focus.focusPointMode = Camera.FocusPointCustom;
        camera.searchAndLock();
    }
}
