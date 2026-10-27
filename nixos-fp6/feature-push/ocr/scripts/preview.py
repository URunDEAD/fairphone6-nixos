#!/usr/bin/env python3
"""Render the actual Lomiri QML with Qt's offscreen platform with Mesa software OpenGL."""
import json
import os
from pathlib import Path
import sys
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ.setdefault("LIBGL_ALWAYS_SOFTWARE", "1")
from PyQt5.QtCore import QTimer, QUrl
from PyQt5.QtWidgets import QApplication
from PyQt5.QtQuick import QQuickView
from PyQt5.QtQml import QQmlComponent

app = QApplication(sys.argv)
view = QQuickView()
view.setResizeMode(QQuickView.SizeRootObjectToView)
view.resize(480, 960)
qml = Path("qml").resolve().as_uri()
component = QQmlComponent(view.engine())
component.setData(f'import QtQuick 2.12\nimport "{qml}" as App\nApp.Main {{ preview: true }}'.encode(), QUrl())
root = component.create()
if root is None:
    raise SystemExit("\n".join(e.toString() for e in component.errors()))
view.setContent(QUrl(), component, root)
view.show()
mode = sys.argv[1] if len(sys.argv) > 1 else "empty"
if mode == "recognized":
    sample = json.loads(Path("evaluation/preview-result.json").read_text())
    sample["image_url"] = Path("evaluation/generated/romanian.png").resolve().as_uri()
    from PyQt5.QtQml import QJSValue
    # Convert through the QML engine so nested arrays keep JavaScript semantics.
    value = view.engine().evaluate("(" + json.dumps(sample, ensure_ascii=False) + ")")
    from PyQt5.QtCore import QMetaObject, Q_ARG
    QMetaObject.invokeMethod(root, "acceptResult", Q_ARG("QVariant", value))
    root.setProperty("language", "ron")
    root.setProperty("chosen", view.engine().evaluate("({'0':true,'1':true,'2':true})"))

def save():
    target = Path("previews") / (mode + ".png")
    target.parent.mkdir(exist_ok=True)
    if not view.grabWindow().save(str(target)):
        app.exit(1)
    else:
        print(target)
        app.quit()
QTimer.singleShot(1800, save)
sys.exit(app.exec_())
