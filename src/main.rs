mod filtering;
mod keyboard_retrieval;

use std::convert::Infallible;
use std::io::{self, ErrorKind};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use evdev::Device;
use log::LevelFilter;

use crate::filtering::filter_chattering;
use crate::keyboard_retrieval::{abs_keyboard_path, retrieve_keyboard_name};

#[derive(Parser)]
#[command(version, about = "Filters mechanical keyboard chattering on Linux")]
struct Args {
    /// Name of your chattering keyboard device as listed in /dev/input/by-id.
    /// If left unset, will be attempted to be retrieved automatically.
    #[arg(short, long, value_name = "KEYBOARD")]
    keyboard: Option<String>,

    /// Filter time threshold in milliseconds.
    #[arg(short, long, value_name = "THRESHOLD", default_value_t = 30)]
    threshold: u64,

    /// 0: silent, 1: log filtered events, 2: log all events.
    #[arg(
        short,
        long,
        default_value_t = 1,
        value_parser = clap::value_parser!(u8).range(0..=2)
    )]
    verbosity: u8,
}

fn main() -> ExitCode {
    let args = Args::parse();
    init_logging(args.verbosity);

    match run(&args) {
        Ok(never) => match never {},
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<Infallible> {
    let keyboard = match args.keyboard.as_deref().filter(|k| !k.is_empty()) {
        Some(name) => name.to_owned(),
        None => retrieve_keyboard_name()?,
    };
    let device = open_device(&keyboard)?;
    filter_chattering(device, args.threshold)
}

fn open_device(keyboard: &str) -> Result<Device> {
    let path = abs_keyboard_path(keyboard);
    Device::open(&path).map_err(|e| {
        let hint = match e.kind() {
            ErrorKind::PermissionDenied => " (are you running as root?)",
            ErrorKind::NotFound => " (see 'ls /dev/input/by-id/' for available devices)",
            _ => "",
        };
        io::Error::new(e.kind(), format!("{e}{hint}"))
    })
    .with_context(|| format!("Couldn't open '{}'", path.display()))
}

fn init_logging(verbosity: u8) {
    let level = match verbosity {
        0 => LevelFilter::Off,
        1 => LevelFilter::Info,
        _ => LevelFilter::Debug,
    };

    env_logger::Builder::new()
        .filter_level(level)
        .target(env_logger::Target::Stdout)
        .format(|buf, record| {
            use std::io::Write;
            writeln!(
                buf,
                "{} - {}",
                chrono::Local::now().format("%H:%M:%S"),
                record.args()
            )
        })
        .init();
}
