// UsbDrive: the only C++ type, a thin QObject over the Rust backend.
//
// Qt 5.15 QML types must be C++ QObjects (moc), so this shim only forwards
// call(name, args) to fp6_usbdrive_command as JSON and parses the reply.
// Library scanning, checksums, downloads, image creation and the D-Bus
// calls to the privileged helper are all in Rust (rust/src/ops.rs).
#pragma once
#include <QObject>
#include <QVariant>

struct Fp6UsbDrive;

class UsbDrive : public QObject {
    Q_OBJECT
public:
    explicit UsbDrive(QObject *parent = nullptr);
    ~UsbDrive() override;
    Q_INVOKABLE QVariant call(const QString &name, const QVariantMap &arg = QVariantMap());

private:
    Fp6UsbDrive *m_handle;
};
