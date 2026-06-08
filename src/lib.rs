#![deny(missing_docs)]

//! # dewpoint
//!
//! Dewpoint is a Prometheus exporter written in Rust that monitors Govee H5075 Bluetooth
//! thermometer/hygrometer sensors. The H5075 broadcasts its readings inside its BLE advertisement
//! data, so dewpoint listens passively and never connects to the device. This means scraping has no
//! effect on the sensor's battery life.

use clap::Parser;
use log::debug;
use metrics::{describe_gauge, gauge};
use std::net::{IpAddr, Ipv4Addr};

/// Bluetooth SIG company identifier used by Govee H5075 advertisements.
pub const GOVEE_COMPANY_ID: u16 = 0xEC88;

/// Default configuration options
const DEFAULT_BIND_IP: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
const DEFAULT_BIND_PORT: u16 = 9185;
const DEFAULT_STALE_AFTER: u64 = 300;

/// Minimum plausible temperature in Celsius, used to reject corrupt advertisements.
const MIN_TEMP_C: f64 = -40.0;
/// Maximum plausible temperature in Celsius, used to reject corrupt advertisements.
const MAX_TEMP_C: f64 = 100.0;

/// Prometheus metric names exported by dewpoint.
const METRIC_TEMPERATURE_C: &str = "dewpoint_temperature_celsius";
const METRIC_TEMPERATURE_F: &str = "dewpoint_temperature_fahrenheit";
const METRIC_HUMIDITY: &str = "dewpoint_humidity_percent";
const METRIC_BATTERY: &str = "dewpoint_battery_percent";
const METRIC_RSSI: &str = "dewpoint_rssi_dbm";
const METRIC_LAST_SEEN: &str = "dewpoint_last_seen_timestamp_seconds";

/// A collection of arguments to be parsed from the command line or environment.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// IP address on which the exporter will serve metrics. Default is `0.0.0.0`.
    #[arg(long, env, default_value_t = DEFAULT_BIND_IP)]
    pub bind_ip: IpAddr,
    /// Port on which the exporter will serve metrics. Default is `9185`.
    #[arg(long, env, default_value_t = DEFAULT_BIND_PORT)]
    pub bind_port: u16,
    /// Optional allowlist of sensor MAC addresses to export. May be repeated or comma-separated.
    /// When empty, every discovered H5075 is exported. Default is empty.
    #[arg(long, env, value_delimiter = ',')]
    pub address: Vec<String>,
    /// Time in seconds after which a sensor that stops broadcasting has its metrics expired.
    /// Must be at least 1 second. Default is `300`.
    #[arg(long, env, default_value_t = DEFAULT_STALE_AFTER, value_parser = clap::value_parser!(u64).range(1..))]
    pub stale_after: u64,
}

/// A single decoded reading from a Govee H5075 advertisement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// Temperature in degrees Celsius.
    pub temp_c: f64,
    /// Relative humidity as a percentage.
    pub humidity: f64,
    /// Battery charge as a percentage.
    pub battery: u8,
    /// Whether the sensor flagged the reading as an error.
    pub error: bool,
}

impl Reading {
    /// Returns the temperature converted to degrees Fahrenheit.
    #[must_use]
    pub fn temp_f(&self) -> f64 {
        self.temp_c * 9.0 / 5.0 + 32.0
    }

    /// Returns `true` if the temperature is within the sensor's plausible operating range and the
    /// error flag is not set.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.error && self.temp_c >= MIN_TEMP_C && self.temp_c <= MAX_TEMP_C
    }
}

/// Decodes the six-byte Govee H5075 manufacturer-data payload into a [`Reading`].
///
/// The first byte is a constant prefix. Bytes one through three pack the temperature and humidity
/// into a single integer, and byte four packs the battery percentage alongside an error flag in its
/// most significant bit. Returns `None` if the payload is not exactly six bytes long.
#[must_use]
pub fn decode_h5075(data: &[u8]) -> Option<Reading> {
    let [_, b1, b2, b3, b4, _] = *<&[u8; 6]>::try_from(data).ok()?;

    let packed = (u32::from(b1) << 16) | (u32::from(b2) << 8) | u32::from(b3);
    let is_negative = packed & 0x0080_0000 != 0;
    let value = packed & 0x007F_FFFF;

    let mut temp_c = f64::from(value / 1000) / 10.0;
    if is_negative {
        temp_c = -temp_c;
    }
    let humidity = f64::from(value % 1000) / 10.0;
    let battery = b4 & 0x7F;
    let error = b4 & 0x80 != 0;

    Some(Reading { temp_c, humidity, battery, error })
}

/// Returns `true` if a sensor address should be exported, given an optional allowlist.
///
/// An empty allowlist matches every address. Comparison is case-insensitive.
#[must_use]
pub fn is_allowed(address: &str, allowlist: &[String]) -> bool {
    allowlist.is_empty() || allowlist.iter().any(|allowed| allowed.eq_ignore_ascii_case(address))
}

/// Registers descriptions for all gauges exported by dewpoint.
pub fn describe_metrics() {
    describe_gauge!(METRIC_TEMPERATURE_C, metrics::Unit::Count, "Sensor temperature in degrees Celsius");
    describe_gauge!(METRIC_TEMPERATURE_F, metrics::Unit::Count, "Sensor temperature in degrees Fahrenheit");
    describe_gauge!(METRIC_HUMIDITY, metrics::Unit::Percent, "Sensor relative humidity as a percentage");
    describe_gauge!(METRIC_BATTERY, metrics::Unit::Percent, "Sensor battery charge as a percentage");
    describe_gauge!(METRIC_RSSI, metrics::Unit::Count, "Received signal strength of the last advertisement in dBm");
    describe_gauge!(METRIC_LAST_SEEN, metrics::Unit::Seconds, "Unix timestamp of the last advertisement received");
}

/// Updates all gauges for a single sensor from a decoded [`Reading`].
///
/// Each gauge is labelled with the sensor's `address` and `name` so multiple sensors can be
/// exported simultaneously. The `rssi` is omitted when it is unavailable, and `last_seen` is the
/// Unix timestamp, in seconds, at which the advertisement was processed.
pub fn record_reading(address: &str, name: &str, rssi: Option<i16>, reading: &Reading, last_seen: f64) {
    gauge!(METRIC_TEMPERATURE_C, "address" => address.to_owned(), "name" => name.to_owned()).set(reading.temp_c);
    gauge!(METRIC_TEMPERATURE_F, "address" => address.to_owned(), "name" => name.to_owned()).set(reading.temp_f());
    gauge!(METRIC_HUMIDITY, "address" => address.to_owned(), "name" => name.to_owned()).set(reading.humidity);
    gauge!(METRIC_BATTERY, "address" => address.to_owned(), "name" => name.to_owned()).set(f64::from(reading.battery));
    gauge!(METRIC_LAST_SEEN, "address" => address.to_owned(), "name" => name.to_owned()).set(last_seen);
    if let Some(rssi) = rssi {
        gauge!(METRIC_RSSI, "address" => address.to_owned(), "name" => name.to_owned()).set(f64::from(rssi));
    }

    debug!(
        "Recorded {address} ({name}): {:.1}C / {:.1}% RH / batt {}%",
        reading.temp_c, reading.humidity, reading.battery
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_args() {
        let args = Args::parse_from(["dewpoint"]);
        assert_eq!(args.bind_ip, DEFAULT_BIND_IP);
        assert_eq!(args.bind_port, DEFAULT_BIND_PORT);
        assert_eq!(args.stale_after, DEFAULT_STALE_AFTER);
        assert!(args.address.is_empty());
    }

    #[test]
    fn parse_override_args() {
        let args = Args::parse_from([
            "dewpoint",
            "--bind-ip",
            "10.0.0.1",
            "--bind-port",
            "1234",
            "--address",
            "A4:C1:38:7A:93:C3,AA:BB:CC:DD:EE:FF",
            "--stale-after",
            "60",
        ]);
        assert_eq!(args.bind_ip, IpAddr::V4("10.0.0.1".parse().unwrap()));
        assert_eq!(args.bind_port, 1234);
        assert_eq!(args.stale_after, 60);
        assert_eq!(args.address, vec!["A4:C1:38:7A:93:C3".to_string(), "AA:BB:CC:DD:EE:FF".to_string()]);
    }

    #[test]
    fn decode_known_payload() {
        // 0003a3e53b00 -> 23.8C, 56.5%, battery 59%
        let reading = decode_h5075(&[0x00, 0x03, 0xa3, 0xe5, 0x3b, 0x00]).unwrap();
        assert!((reading.temp_c - 23.8).abs() < f64::EPSILON);
        assert!((reading.humidity - 56.5).abs() < f64::EPSILON);
        assert_eq!(reading.battery, 59);
        assert!(!reading.error);
        assert!(reading.is_valid());
    }

    #[test]
    fn decode_temperature_to_fahrenheit() {
        let reading = decode_h5075(&[0x00, 0x03, 0x41, 0x8a, 0x64, 0x00]).unwrap();
        assert!((reading.temp_c - 21.3).abs() < f64::EPSILON);
        assert!((reading.temp_f() - 70.34).abs() < 1e-9);
        assert_eq!(reading.battery, 100);
    }

    #[test]
    fn decode_negative_temperature() {
        let reading = decode_h5075(&[0x00, 0x80, 0x12, 0xd6, 0x55, 0x00]).unwrap();
        assert!(reading.temp_c < 0.0);
        assert!((reading.temp_c - -0.4).abs() < f64::EPSILON);
        assert!((reading.humidity - 82.2).abs() < f64::EPSILON);
        assert_eq!(reading.battery, 85);
    }

    #[test]
    fn decode_error_flag() {
        // High bit of the battery byte signals an error reading.
        let reading = decode_h5075(&[0x00, 0x03, 0xa3, 0xe5, 0xbb, 0x00]).unwrap();
        assert!(reading.error);
        assert_eq!(reading.battery, 59);
        assert!(!reading.is_valid());
    }

    #[test]
    fn decode_rejects_wrong_length() {
        assert!(decode_h5075(&[0x00, 0x03, 0xa3]).is_none());
        assert!(decode_h5075(&[]).is_none());
        assert!(decode_h5075(&[0x00, 0x03, 0xa3, 0xe5, 0x3b, 0x00, 0x00]).is_none());
    }

    #[test]
    fn allowlist_empty_matches_everything() {
        assert!(is_allowed("A4:C1:38:7A:93:C3", &[]));
    }

    #[test]
    fn allowlist_matches_case_insensitively() {
        let allow = vec!["a4:c1:38:7a:93:c3".to_string()];
        assert!(is_allowed("A4:C1:38:7A:93:C3", &allow));
    }

    #[test]
    fn allowlist_rejects_unlisted_address() {
        let allow = vec!["AA:BB:CC:DD:EE:FF".to_string()];
        assert!(!is_allowed("A4:C1:38:7A:93:C3", &allow));
    }
}
