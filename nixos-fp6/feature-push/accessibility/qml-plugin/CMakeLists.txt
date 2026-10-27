# SPDX-License-Identifier: GPL-3.0-only
cmake_minimum_required(VERSION 3.16)
project(fp6-accessibility-qml LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick)
set(QML_INSTALL_DIR "lib/qt-5/qml" CACHE STRING "QML module root")
option(FP6_BUILD_TESTS "build the unit tests" OFF)

set(SOURCES colourmath.cpp colourmath.h colourmatrix.cpp colourmatrix.h magnifier.cpp magnifier.h)
add_library(fp6accessibility-qml MODULE plugin.cpp ${SOURCES})
target_compile_definitions(fp6accessibility-qml PRIVATE QT_NO_KEYWORDS)
target_link_libraries(fp6accessibility-qml PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick)

install(TARGETS fp6accessibility-qml DESTINATION ${QML_INSTALL_DIR}/FP6/Accessibility)
install(FILES qmldir DESTINATION ${QML_INSTALL_DIR}/FP6/Accessibility)

if(FP6_BUILD_TESTS)
  find_package(Qt5 5.15 REQUIRED COMPONENTS Test)
  add_executable(colourmath-test tests/colourmath_test.cpp colourmath.cpp colourmath.h)
  target_include_directories(colourmath-test PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
  add_executable(magnifier-test tests/magnifier_test.cpp ${SOURCES})
  target_compile_definitions(magnifier-test PRIVATE QT_NO_KEYWORDS)
  target_include_directories(magnifier-test PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
  target_link_libraries(magnifier-test PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick Qt5::Test)
endif()
