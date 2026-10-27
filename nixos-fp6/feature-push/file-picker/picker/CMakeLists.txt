cmake_minimum_required(VERSION 3.16)
project(fp6-file-picker LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_AUTOMOC ON)
find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick)

set(FP6_PICKER_QML_INSTALL_DIR "${CMAKE_INSTALL_PREFIX}/share/fp6-file-picker/qml")

add_executable(fp6-file-picker main.cpp)
target_compile_definitions(fp6-file-picker PRIVATE FP6_PICKER_QML_DIR="${FP6_PICKER_QML_INSTALL_DIR}")
target_link_libraries(fp6-file-picker PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick)

install(TARGETS fp6-file-picker RUNTIME DESTINATION bin)
install(DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/../qml/ DESTINATION share/fp6-file-picker/qml)
