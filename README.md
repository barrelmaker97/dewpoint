# Dewpoint
**Dewpoint** is a Prometheus exporter for [Govee H5075](https://www.govee.com/) Bluetooth thermometer/hygrometer sensors.

The H5075 broadcasts its temperature, humidity, and battery level inside its Bluetooth Low Energy advertisement data. Dewpoint listens for these broadcasts passively and never connects to the sensor, so exporting metrics has **no effect on the sensor's battery life**. Multiple sensors are supported simultaneously and exported as separate label sets.

## Features

- **Passive & Battery-Friendly**: Reads broadcast advertisements without ever connecting to the device, so it does not drain the sensor's battery.
- **Multi-Sensor**: Automatically discovers and exports every H5075 in range, labelled by address and name.
- **Prometheus Integration**: Exposes temperature, humidity, battery, and signal strength gauges on an HTTP endpoint.
- **Stale Sensor Expiry**: Metrics for sensors that stop broadcasting are automatically expired.
- **Configurable**: All settings can be customized via command-line or environment variables.

## Configuration

Dewpoint can be configured using either command-line options or by setting corresponding environment variables.
Command-line options take precedence over environment variables.
Below is a breakdown of the available options:

| Option                        | Description                                                                              | Environment Variable | Default   |
|-------------------------------|------------------------------------------------------------------------------------------|----------------------|-----------|
| `--bind-ip <BIND_IP>`         | IP address on which the exporter will serve metrics.                                     | `BIND_IP`            | `0.0.0.0` |
| `--bind-port <BIND_PORT>`     | Port on which the exporter will serve metrics.                                           | `BIND_PORT`          | `9185`    |
| `--address <ADDRESS>`         | Optional allowlist of sensor MAC addresses to export. May be repeated or comma-separated. When empty, every discovered H5075 is exported. | `ADDRESS` | (all)     |
| `--stale-after <STALE_AFTER>` | Time in seconds after which a sensor that stops broadcasting has its metrics expired. Must be at least 1 second. | `STALE_AFTER` | `300`     |
| `-h, --help`                  | Print help message                                                                       | -                    | -         |
| `-V, --version`               | Print version information                                                                | -                    | -         |

### Example

To run Dewpoint while restricting it to a single sensor, you can either use the command-line options:

```bash
dewpoint --address A4:C1:38:7A:93:C3
```

Or set the environment variables:

```bash
export ADDRESS=A4:C1:38:7A:93:C3
dewpoint
```

If Dewpoint is run as a systemd service, `systemctl edit` can be used to set configuration options.
Run the following to open up an `override.conf` file in `/etc/systemd/system/dewpoint.service.d/`
```bash
sudo systemctl edit dewpoint.service
```

In this file, environment variables can be set to configure Dewpoint:
```
[Service]
Environment="ADDRESS=A4:C1:38:7A:93:C3"
Environment="STALE_AFTER=120"
```

After saving the file, the service must be restarted for changes to take effect:
```bash
sudo systemctl restart dewpoint.service
```

## Exported Metrics

All metrics are gauges labelled with `address` (the sensor MAC) and `name` (the advertised local name, e.g. `GVH5075_93C3`).

| Metric                                   | Description                                              |
|------------------------------------------|----------------------------------------------------------|
| `dewpoint_temperature_celsius`           | Sensor temperature in degrees Celsius.                   |
| `dewpoint_temperature_fahrenheit`        | Sensor temperature in degrees Fahrenheit.                |
| `dewpoint_humidity_percent`              | Sensor relative humidity as a percentage.                |
| `dewpoint_battery_percent`               | Sensor battery charge as a percentage.                   |
| `dewpoint_rssi_dbm`                      | Received signal strength of the last advertisement, dBm. |
| `dewpoint_last_seen_timestamp_seconds`   | Unix timestamp of the last advertisement received.       |

### Example Scrape

```
dewpoint_temperature_celsius{address="A4:C1:38:7A:93:C3",name="GVH5075_93C3"} 24.1
dewpoint_temperature_fahrenheit{address="A4:C1:38:7A:93:C3",name="GVH5075_93C3"} 75.38
dewpoint_humidity_percent{address="A4:C1:38:7A:93:C3",name="GVH5075_93C3"} 58.1
dewpoint_battery_percent{address="A4:C1:38:7A:93:C3",name="GVH5075_93C3"} 59
dewpoint_rssi_dbm{address="A4:C1:38:7A:93:C3",name="GVH5075_93C3"} -42
dewpoint_last_seen_timestamp_seconds{address="A4:C1:38:7A:93:C3",name="GVH5075_93C3"} 1780955454
```

> [!TIP]
> Because a sensor that stops broadcasting has its metrics expired after `--stale-after` seconds, you can alert on an offline sensor by checking whether its series is `absent()` in Prometheus.

## Building

Dewpoint depends on BlueZ via system D-Bus, so the D-Bus development headers are required to build:

```bash
sudo apt install pkg-config libdbus-1-dev
cargo build --release
```

## Running with Docker

A container image is published to the GitHub Container Registry. Because dewpoint needs access to the host's Bluetooth adapter (via BlueZ on the system D-Bus), the container must share the host's D-Bus and network:

```bash
docker run -d \
  --name dewpoint \
  --net=host \
  -v /var/run/dbus:/var/run/dbus \
  ghcr.io/barrelmaker97/dewpoint:latest
```

## Logging

Log verbosity is controlled with the `RUST_LOG` environment variable (e.g. `RUST_LOG=debug`). The default level is `info`.

## License

Licensed under the [GNU General Public License v3.0 or later](LICENSE).
