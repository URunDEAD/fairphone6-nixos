# SPDX-License-Identifier: GPL-3.0-only
cmake_minimum_required(VERSION 3.16)
project(fp6-screenrecorder-qml LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick DBus)
find_package(Threads REQUIRED)

set(FP6_SCREENRECORD_BIN "fp6-screenrecord" CACHE STRING "recorder executable")
set(FP6_RECORD_WIDTH 720 CACHE STRING "video width (portrait)")
set(FP6_RECORD_FPS 30 CACHE STRING "maximum frame rate")
set(QML_INSTALL_DIR "lib/qt-5/qml" CACHE STRING "QML module root")

add_library(fp6screenrecorder-qml MODULE plugin.cpp screenrecorder.cpp screenrecorder.h)
target_compile_definitions(fp6screenrecorder-qml PRIVATE
    FP6_SCREENRECORD_BIN="${FP6_SCREENRECORD_BIN}"
    FP6_RECORD_WIDTH=${FP6_RECORD_WIDTH}
    FP6_RECORD_FPS=${FP6_RECORD_FPS}
    QT_NO_KEYWORDS)
target_link_libraries(fp6screenrecorder-qml PRIVATE
    Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick Qt5::DBus Threads::Threads)

install(TARGETS fp6screenrecorder-qml DESTINATION ${QML_INSTALL_DIR}/FP6/ScreenRecorder)
install(FILES qmldir DESTINATION ${QML_INSTALL_DIR}/FP6/ScreenRecorder)
