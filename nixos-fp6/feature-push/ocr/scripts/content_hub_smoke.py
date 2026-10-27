#!/usr/bin/env python3
"""Exercise native receiver ordering: state can precede items, or already be collected."""
from pathlib import Path
import sys
from PyQt5.QtCore import QMetaObject, QUrl
from PyQt5.QtWidgets import QApplication
from PyQt5.QtQml import QQmlEngine, QQmlComponent
app = QApplication(sys.argv)
engine = QQmlEngine()
component = QQmlComponent(engine)
source = '''
import QtQuick 2.12
import Lomiri.Content 1.3
import "QML_PATH" as App
Item {
    property int received: 0
    property bool passed: false
    App.ContentActions { id: receiver; onImageReceived: parent.received += 1 }
    QtObject { id: early; property int state: ContentTransfer.Charged; property var items: [] }
    QtObject { id: collected; property int state: ContentTransfer.Collected; property var items: [{url:"file:///synthetic/second.png"}] }
    function check() {
        receiver.receive(early);
        if (received !== 0) return;
        early.items = [{url:"file:///synthetic/first.png"}];
        if (received !== 1) return;
        early.state = ContentTransfer.Collected;
        receiver.collect();
        if (received !== 1) return;
        receiver.receive(collected);
        if (received !== 2) return;
        passed = true;
    }
}
'''.replace('QML_PATH', Path('qml').resolve().as_uri())
component.setData(source.encode(), QUrl())
root = component.create()
assert root is not None, '\n'.join(e.toString() for e in component.errors())
QMetaObject.invokeMethod(root, 'check')
assert root.property('passed'), f"Unexpected delivery count: {root.property('received')}"
print('Content Hub receiver ordering and duplicate suppression: PASS')
