# Out-of-tree build of the patched QtSensors iio-sensor-proxy backend
# (orientation only, identifier fp6-iio-sensor-proxy.orientationsensor).
TEMPLATE = lib
CONFIG += plugin c++14 hide_symbols
TARGET = qtsensors_fp6-iio-sensor-proxy
QT = core dbus sensors

HEADERS += iiosensorproxysensorbase.h iiosensorproxyorientationsensor.h
SOURCES += iiosensorproxysensorbase.cpp iiosensorproxyorientationsensor.cpp main.cpp
OTHER_FILES += plugin.json

DBUS_INTERFACES += sensor_proxy dbus_properties
sensor_proxy.files = net.hadess.SensorProxy.xml
sensor_proxy.header_flags = -N
dbus_properties.files = org.freedesktop.DBus.Properties.xml
dbus_properties.header_flags = -N

LIBS += -lrt
target.path = $$PREFIX/sensors
INSTALLS += target
