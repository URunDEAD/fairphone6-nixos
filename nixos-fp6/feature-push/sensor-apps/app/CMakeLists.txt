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

# Magnifier: same launcher, fixed tool, own binary (camera env + AppArmor).
add_executable(fp6-magnifier main.cpp sensorfeed.cpp sensorfeed.h fp6_sensors.h)
target_compile_definitions(fp6-magnifier PRIVATE FP6_QML_DIR="${FP6_QML_INSTALL_DIR}" FP6_FIXED_TOOL="magnifier")
target_link_libraries(fp6-magnifier PRIVATE Qt5::Core Qt5::Gui Qt5::Qml Qt5::Quick
    ${FP6_SENSORS_LIB} Threads::Threads ${CMAKE_DL_LIBS} m)

install(TARGETS fp6-sensor-tools fp6-magnifier RUNTIME DESTINATION bin)
install(DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/../qml/ DESTINATION share/fp6-sensor-apps/qml)
