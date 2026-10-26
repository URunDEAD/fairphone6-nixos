//! fp6-sensors: backend of the FP6 Lomiri sensor apps (Compass, Level,
//! Measure, Altimeter, Sound meter, Light meter, GPS status, Sensors).
//!
//! The QML UI talks to it through the C ABI in `ffi` (a QObject shim in
//! app/sensorfeed.cpp). All sensor access, maths and state live here.

pub mod acoustics;
pub mod baro;
pub mod coords;
pub mod ffi;
pub mod iio;
pub mod nmea;
pub mod orient;
pub mod panel;
pub mod photo;
pub mod tools;
pub mod util;
pub mod wmm;
