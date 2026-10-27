#!/usr/bin/env python3
"""Invoked in place of qmlscene to exercise the real per-window bridge."""
import os
import sys
from pathlib import Path
from PyQt5.QtCore import QTimer, QUrl, QMetaObject, Q_ARG, QObject
from PyQt5.QtWidgets import QApplication
from PyQt5.QtQuick import QQuickView
app = QApplication(sys.argv)
view = QQuickView()
view.setResizeMode(QQuickView.SizeRootObjectToView)
view.resize(480, 960)
view.setSource(QUrl.fromLocalFile(sys.argv[1]))
if view.status() == QQuickView.Error:
    raise SystemExit("\n".join(e.toString() for e in view.errors()))
view.show()
root = view.rootObject()

def inspect():
    if root.property("busy") or not root.property("result"):
        return
    assert root.findChild(QObject, "recognizedText").property("textFormat") == 0, "OCR text must be plain text"
    QMetaObject.invokeMethod(root, "copyAll")
    if "România" not in app.clipboard().text():
        print("Unexpected clipboard: " + app.clipboard().text(), file=sys.stderr)
        app.exit(1)
        return
    QMetaObject.invokeMethod(root, "toggleWord", Q_ARG("QVariant", 0))
    QMetaObject.invokeMethod(root, "copySelected")
    if app.clipboard().text() != "În":
        print("Selection copy failed", file=sys.stderr)
        app.exit(1)
        return
    Path("previews").mkdir(exist_ok=True)
    assert view.grabWindow().save("previews/live-recognition.png")
    print("Live QML + HTTP bridge + D-Bus + OCR + clipboard: PASS")
    app.quit()
timer = QTimer()
timer.timeout.connect(inspect)
timer.start(500)
QTimer.singleShot(90000, lambda: (print("UI timed out: " + str(root.property("message")), file=sys.stderr), app.exit(2)))
sys.exit(app.exec_())
