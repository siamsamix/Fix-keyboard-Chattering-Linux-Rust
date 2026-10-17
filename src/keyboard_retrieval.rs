use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use log::info;

pub const INPUT_DEVICES_PATH: &str = "/dev/input/by-id";

/// Returns the name of the keyboard device inside [`INPUT_DEVICES_PATH`], asking
/// the user to pick one if there are several candidates.
pub fn retrieve_keyboard_name() -> Result<String> {
    let mut devices: Vec<String> = fs::read_dir(INPUT_DEVICES_PATH)
        .with_context(|| format!("Couldn't list '{INPUT_DEVICES_PATH}'"))?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    devices.sort();
    devices.dedup();

    match devices.len() {
        0 => bail!("Couldn't find a keyboard in '{INPUT_DEVICES_PATH}'"),
        1 => {
            let device = devices.remove(0);
            info!("Found keyboard: {device}");
            Ok(device)
        }
        _ => {
            let idx = prompt_for_choice(&devices)?;
            Ok(devices.swap_remove(idx))
        }
    }
}

pub fn abs_keyboard_path(device: &str) -> PathBuf {
    PathBuf::from(INPUT_DEVICES_PATH).join(device)
}

/// Lets the user pick one of `devices`, returning the index of the selection.
fn prompt_for_choice(devices: &[String]) -> Result<usize> {
    println!("Select a device:");
    for (i, device) in devices.iter().enumerate() {
        println!("{}. {device}", i + 1);
    }

    let stdin = io::stdin();
    loop {
        print!("Enter your choice (number): ");
        io::stdout().flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            bail!("No device selected: stdin was closed. Pass one explicitly using -k.");
        }

        match line.trim().parse::<usize>() {
            Ok(n) if (1..=devices.len()).contains(&n) => return Ok(n - 1),
            Ok(_) => println!("Please select a number between 1 and {}", devices.len()),
            Err(_) => println!("Please enter a valid number"),
        }
    }
}
