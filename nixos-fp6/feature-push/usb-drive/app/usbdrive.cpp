#include "usbdrive.h"
#include "fp6_usbdrive.h"

#include <QJsonDocument>

UsbDrive::UsbDrive(QObject *parent) : QObject(parent), m_handle(fp6_usbdrive_new()) {}

UsbDrive::~UsbDrive()
{
    fp6_usbdrive_free(m_handle);
}

QVariant UsbDrive::call(const QString &name, const QVariantMap &arg)
{
    const QByteArray a = QJsonDocument::fromVariant(arg).toJson(QJsonDocument::Compact);
    char *r = fp6_usbdrive_command(m_handle, name.toUtf8().constData(), a.constData());
    QVariant v;
    if (r) {
        v = QJsonDocument::fromJson(QByteArray(r)).toVariant();
        fp6_usbdrive_string_free(r);
    }
    return v;
}
