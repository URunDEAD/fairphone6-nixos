cmake_minimum_required(VERSION 3.16)
project(fp6-wifi-qr LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_AUTOMOC ON)
option(FP6_BUILD_PLUGIN "Fp6.WifiQr QML module for System Settings" ON)
option(FP6_BUILD_SCANNER "fp6-wifi-qr-scan app (needs QtMultimedia)" ON)
set(FP6_WIFIQR_LIB "" CACHE FILEPATH "libfp6_wifi_qr.a from the Rust crate")
set(FP6_PLUGIN_DIR "lib/lomiri-system-settings/private/Fp6/WifiQr" CACHE STRING "QML module install dir")
set(FP6_QML_INSTALL_DIR "${CMAKE_INSTALL_PREFIX}/share/fp6-wifi-qr/qml")

find_package(Threads REQUIRED)
set(COMMON common/qrcodeitem.cpp common/qrcodeitem.h common/nmutil.cpp common/nmutil.h common/fp6_wifi_qr.h)
set(RUST_LIBS ${FP6_WIFIQR_LIB} Threads::Threads ${CMAKE_DL_LIBS} m)

if(FP6_BUILD_PLUGIN)
    find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick DBus Concurrent)
    find_library(PAM_LIB pam REQUIRED)
    add_library(fp6wifiqrplugin MODULE plugin/plugin.cpp plugin/wifishare.cpp plugin/wifishare.h ${COMMON})
    target_link_libraries(fp6wifiqrplugin PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick Qt5::DBus Qt5::Concurrent
        ${PAM_LIB} ${RUST_LIBS})
    install(TARGETS fp6wifiqrplugin DESTINATION ${FP6_PLUGIN_DIR})
    install(FILES plugin/qmldir DESTINATION ${FP6_PLUGIN_DIR})
endif()

if(FP6_BUILD_SCANNER)
    find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick DBus Concurrent Multimedia)
    add_executable(fp6-wifi-qr-scan scanner/main.cpp scanner/qrscanfilter.cpp scanner/qrscanfilter.h
        scanner/wifijoin.cpp scanner/wifijoin.h ${COMMON})
    target_compile_definitions(fp6-wifi-qr-scan PRIVATE FP6_QML_DIR="${FP6_QML_INSTALL_DIR}")
    target_link_libraries(fp6-wifi-qr-scan PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick Qt5::DBus Qt5::Concurrent
        Qt5::Multimedia ${RUST_LIBS})
    install(TARGETS fp6-wifi-qr-scan RUNTIME DESTINATION bin)
    install(DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/../scanner-qml/ DESTINATION share/fp6-wifi-qr/qml)
endif()
