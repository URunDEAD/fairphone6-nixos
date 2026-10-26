# SPDX-License-Identifier: GPL-3.0-only
cmake_minimum_required(VERSION 3.16)
project(fp6-displaycomfort-qml LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick)
set(QML_INSTALL_DIR "lib/qt-5/qml" CACHE STRING "QML module root")
option(FP6_BUILD_TESTS "build the offscreen pixel test" OFF)

add_library(fp6displaycomfort-qml MODULE plugin.cpp colorfilter.cpp colorfilter.h)
target_compile_definitions(fp6displaycomfort-qml PRIVATE QT_NO_KEYWORDS)
target_link_libraries(fp6displaycomfort-qml PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick)

install(TARGETS fp6displaycomfort-qml DESTINATION ${QML_INSTALL_DIR}/FP6/DisplayComfort)
install(FILES qmldir DESTINATION ${QML_INSTALL_DIR}/FP6/DisplayComfort)

if(FP6_BUILD_TESTS)
  add_executable(colorfilter-test tests/colorfilter_test.cpp colorfilter.cpp colorfilter.h)
  target_include_directories(colorfilter-test PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
  target_link_libraries(colorfilter-test PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick)
endif()
