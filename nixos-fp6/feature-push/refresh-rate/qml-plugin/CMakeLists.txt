# SPDX-License-Identifier: GPL-3.0-only
cmake_minimum_required(VERSION 3.16)
project(fp6-refreshrate-qml LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick)
set(QML_INSTALL_DIR "lib/qt-5/qml" CACHE STRING "QML module root")
option(FP6_BUILD_TESTS "build the offscreen test" OFF)

set(SOURCES panelrate.cpp panelrate.h activitywatcher.cpp activitywatcher.h)

add_library(fp6refreshrate-qml MODULE plugin.cpp ${SOURCES})
target_compile_definitions(fp6refreshrate-qml PRIVATE QT_NO_KEYWORDS)
target_compile_options(fp6refreshrate-qml PRIVATE -Wall -Wextra)
target_link_libraries(fp6refreshrate-qml PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick)

install(TARGETS fp6refreshrate-qml DESTINATION ${QML_INSTALL_DIR}/FP6/RefreshRate)
install(FILES qmldir DESTINATION ${QML_INSTALL_DIR}/FP6/RefreshRate)

if(FP6_BUILD_TESTS)
  add_executable(refreshrate-test tests/refreshrate_test.cpp ${SOURCES})
  target_compile_definitions(refreshrate-test PRIVATE QT_NO_KEYWORDS)
  target_include_directories(refreshrate-test PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
  target_link_libraries(refreshrate-test PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick)
endif()
