use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use clap::Parser;
use env_logger::{Builder, Env};
use futures::StreamExt;
use log::{debug, error, info};
use metrics_exporter_prometheus::PrometheusBuilder;
use metrics_util::MetricKindMask;
use std::net::SocketAddr;
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::oneshot;

/// Returns the first available Bluetooth adapter on the system.
async fn get_adapter() -> Result<Adapter, btleplug::Error> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    adapters.into_iter().next().ok_or(btleplug::Error::DeviceNotFound)
}

/// Returns the current Unix timestamp in seconds. Returns zero if the system clock is before the
/// Unix epoch.
fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Decodes a single manufacturer-data advertisement and, if it is a valid Govee H5075 reading from
/// an allowed sensor, records it to the Prometheus metrics.
async fn process_advertisement(adapter: &Adapter, id: &PeripheralId, data: &[u8], allowlist: &[String]) {
    let Some(reading) = dewpoint::decode_h5075(data) else { return };
    if !reading.is_valid() {
        debug!("Ignoring invalid reading: {reading:?}");
        return;
    }

    // The advertisement event carries only the payload, so look up the peripheral to resolve its
    // address, name, and signal strength. Properties may not be populated yet immediately after
    // discovery; in that case skip this advertisement and wait for the next broadcast.
    let Ok(peripheral) = adapter.peripheral(id).await else {
        debug!("Failed to look up advertising peripheral; skipping");
        return;
    };
    let Ok(Some(props)) = peripheral.properties().await else {
        debug!("No properties available yet for advertising device; skipping");
        return;
    };
    let address = props.address.to_string();

    if !dewpoint::is_allowed(&address, allowlist) {
        return;
    }
    let name = props.local_name.unwrap_or_default();
    dewpoint::record_reading(&address, &name, props.rssi, &reading, unix_now());
}

/// Scans for BLE advertisements until a shutdown signal is received, recording every Govee H5075
/// reading to the Prometheus metrics.
async fn run_scanner(
    adapter: Adapter,
    args: dewpoint::Args,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), btleplug::Error> {
    let mut events = adapter.events().await?;
    adapter.start_scan(ScanFilter::default()).await?;
    info!("Scanning for Govee H5075 advertisements...");

    tokio::select! {
        () = async {
            while let Some(event) = events.next().await {
                if let CentralEvent::ManufacturerDataAdvertisement { id, manufacturer_data } = event
                    && let Some(data) = manufacturer_data.get(&dewpoint::GOVEE_COMPANY_ID)
                {
                    process_advertisement(&adapter, &id, data, &args.address).await;
                }
            }
        } => {},
        _ = shutdown_rx => {
            info!("Attempting graceful shutdown");
        }
    }

    if let Err(err) = adapter.stop_scan().await {
        error!("Failed to stop BLE scan during shutdown: {err}");
    }
    Ok(())
}

/// Waits for SIGINT or SIGTERM and then sends a shutdown signal.
async fn handle_signals(shutdown_tx: oneshot::Sender<()>) {
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");

    tokio::select! {
        _ = sigint.recv() => {
            debug!("Received SIGINT, sending shutdown signal");
        }
        _ = sigterm.recv() => {
            debug!("Received SIGTERM, sending shutdown signal");
        }
    };

    if shutdown_tx.send(()).is_err() {
        error!("Failed to send shutdown signal: the receiver may have dropped");
    }
}

#[tokio::main]
async fn main() {
    // Initialize logging
    Builder::from_env(Env::default().default_filter_or("info")).init();

    // Parse configuration
    let args = dewpoint::Args::parse();

    // Find a Bluetooth adapter to scan with
    let adapter = get_adapter().await.unwrap_or_else(|err| {
        error!("Could not find a Bluetooth adapter: {err}");
        process::exit(1);
    });

    // Start prometheus exporter, expiring metrics for sensors that stop broadcasting
    let bind_addr = SocketAddr::new(args.bind_ip, args.bind_port);
    PrometheusBuilder::new()
        .with_http_listener(bind_addr)
        .idle_timeout(MetricKindMask::GAUGE, Some(Duration::from_secs(args.stale_after)))
        .install()
        .unwrap_or_else(|err| {
            error!("Failed to create prometheus exporter: {err}");
            process::exit(1);
        });
    dewpoint::describe_metrics();
    info!("Serving metrics on http://{bind_addr}/metrics");

    // Create a channel for shutdown signaling
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // Start watching for signals
    tokio::spawn(handle_signals(shutdown_tx));

    // Start scanning
    if let Err(err) = run_scanner(adapter, args, shutdown_rx).await {
        error!("BLE scanner failed: {err}");
        process::exit(1);
    }

    info!("Shutdown complete, goodbye");
}
