cmake_minimum_required(VERSION 3.16)
project(fp6-sensor-tools LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_AUTOMOC ON)
find_package(Qt5 5.15 REQUIRED COMPONENTS Core Gui Qml Quick)
find_package(Threads REQUIRED)

set(FP6_SENSORS_LIB "" CACHE FILEPATH "libfp6_sensors.a from the Rust crate")
set(FP6_QML_INSTALL_DIR "${CMAKE_INSTALL_PREFIX}/share/fp6-sensor-apps/qml")

add_executable(fp6-sensor-tools main.cpp sensorfeed.cpp sensorfeed.h fp6_sensors.h)
target_compile_definitions(fp6-sensor-tools PRIVATE FP6_QML_DIR="${FP6_QML_INSTALL_DIR}")
target_link_libraries(fp6-sensor-tools PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick
    ${FP6_SENSORS_LIB} Threads::Threads ${CMAKE_DL_LIBS} m)

install(TARGETS fp6-sensor-tools RUNTIME DESTINATION bin)
install(DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/../qml/ DESTINATION share/fp6-sensor-apps/qml)
