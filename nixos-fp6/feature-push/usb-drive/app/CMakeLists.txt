cmake_minimum_required(VERSION 3.16)
project(fp6-usb-drive LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_AUTOMOC ON)
find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick)
find_package(Threads REQUIRED)

set(FP6_USBDRIVE_LIB "" CACHE FILEPATH "libfp6_usbdrive.a from the Rust crate")
set(FP6_QML_INSTALL_DIR "${CMAKE_INSTALL_PREFIX}/share/fp6-usb-drive/qml")

add_executable(fp6-usb-drive main.cpp usbdrive.cpp usbdrive.h fp6_usbdrive.h)
target_compile_definitions(fp6-usb-drive PRIVATE FP6_QML_DIR="${FP6_QML_INSTALL_DIR}")
target_link_libraries(fp6-usb-drive PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick
    ${FP6_USBDRIVE_LIB} Threads::Threads ${CMAKE_DL_LIBS} m)

install(TARGETS fp6-usb-drive RUNTIME DESTINATION bin)
install(DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/../qml/ DESTINATION share/fp6-usb-drive/qml)
