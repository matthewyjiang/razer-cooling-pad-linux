use anyhow::{Context, Result};
use clap::Parser;
use hidapi::HidApi;
use razer_coolingpad_linux::{
    build_off_report, build_set_rpm_report, clamp_rpm, find_thermal_zones, get_max_temp,
    interpolate_curve, load_config, normalize_curve, open_device, percent_to_rpm, read_rpm,
    read_temp, resolve_sensor_paths, sample_config, send_feature_report, ConfigFile,
};
use std::path::PathBuf;
use std::process;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(about = "Razer Laptop Cooling Pad fan curve controller")]
struct Args {
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[arg(long)]
    list_sensors: bool,
    #[arg(long, value_name = "RPM")]
    set_rpm: Option<u16>,
    #[arg(long)]
    off: bool,
    #[arg(long)]
    interval: Option<f64>,
    #[arg(long)]
    sensor: Vec<PathBuf>,
    #[arg(long)]
    hysteresis: Option<f64>,
    #[arg(long)]
    generate_config: bool,
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();

    if args.list_sensors {
        let zones = find_thermal_zones()?;
        if zones.is_empty() {
            println!("No thermal zones found.");
            return Ok(());
        }
        for zone in zones {
            let temp = read_temp(&zone.path).unwrap_or(f64::NAN);
            println!(
                "  {:20}  {:20}  {:6.1} degC  ({})",
                zone.zone,
                zone.name,
                temp,
                zone.path.display()
            );
        }
        return Ok(());
    }

    if args.generate_config {
        println!(
            "{}",
            serde_json::to_string_pretty(&sample_config()).context("failed to serialize config")?
        );
        return Ok(());
    }

    let mut config = match &args.config {
        Some(path) => load_config(path)?,
        None => ConfigFile::default(),
    };
    if let Some(interval) = args.interval {
        config.interval = interval;
    }
    if let Some(hysteresis) = args.hysteresis {
        config.hysteresis = hysteresis;
    }
    normalize_curve(&mut config.curve)?;

    let sensor_paths = resolve_sensor_paths(&args.sensor, &config)?;
    let interval = config.interval.max(0.1);
    let hysteresis = config.hysteresis.max(0.0);

    let mut api = HidApi::new().context("failed to initialize hidapi")?;
    let mut device = open_device(&api)
        .context("Make sure the pad is connected and you have permissions (try sudo).")?;

    println!("Connected to Razer Cooling Pad");

    if args.off {
        send_feature_report(&device, &build_off_report())?;
        println!("Fan control released (pad returns to default behavior).");
        return Ok(());
    }

    if let Some(rpm) = args.set_rpm {
        let rpm = clamp_rpm(rpm);
        send_feature_report(&device, &build_set_rpm_report(rpm))?;
        println!("Fan set to {rpm} RPM.");
        return Ok(());
    }

    println!("Fan curve active (interval={interval}s, hysteresis={hysteresis} degC)");
    println!("Curve points: {:?}", config.curve);
    println!("Sensors: {:?}", sensor_paths);
    println!("Press Ctrl+C to stop (will release fan control on exit).\n");

    let running = Arc::new(AtomicBool::new(true));
    let signal_flag = Arc::clone(&running);
    ctrlc::set_handler(move || {
        signal_flag.store(false, Ordering::SeqCst);
    })
    .context("failed to install signal handler")?;

    let mut last_rpm: Option<u16> = None;
    let mut last_temp: Option<f64> = None;

    while running.load(Ordering::SeqCst) {
        let Some(temp) = get_max_temp(&sensor_paths) else {
            if args.verbose {
                eprintln!("Warning: Could not read any temperature sensor");
            }
            thread::sleep(Duration::from_secs_f64(interval));
            continue;
        };

        let percent = interpolate_curve(&config.curve, temp);
        let mut rpm = percent_to_rpm(percent);

        if let (Some(previous_temp), Some(previous_rpm)) = (last_temp, last_rpm) {
            if (temp - previous_temp).abs() < hysteresis {
                rpm = previous_rpm;
            }
        }

        if Some(rpm) != last_rpm {
            if let Err(error) = send_feature_report(&device, &build_set_rpm_report(rpm)) {
                eprintln!("Device disconnected, attempting reconnect... ({error:#})");
                loop {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_secs(2));
                    if api.refresh_devices().is_err() {
                        continue;
                    }
                    match open_device(&api) {
                        Ok(reconnected) => {
                            device = reconnected;
                            last_rpm = None;
                            println!("Reconnected.");
                            break;
                        }
                        Err(_) => {}
                    }
                }
                continue;
            }
            last_rpm = Some(rpm);
            last_temp = Some(temp);
        }

        if args.verbose {
            let reported_rpm = read_rpm(&device);
            println!(
                "Temp: {:5.1} degC -> {:5.1}% -> {} RPM (reported: {:?})",
                temp, percent, rpm, reported_rpm
            );
        }

        thread::sleep(Duration::from_secs_f64(interval));
    }

    match send_feature_report(&device, &build_off_report()) {
        Ok(()) => println!("\nFan control released."),
        Err(error) => eprintln!("Failed to release fan control: {error:#}"),
    }
    Ok(())
}
