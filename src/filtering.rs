use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use evdev::uinput::VirtualDevice;
use evdev::{Device, EventType, InputEvent, KeyCode, UinputAbsSetup};
use log::{debug, info};

/// Values of an `EV_KEY` event: 0 for up, 1 for down and 2 for hold.
const KEY_UP: i32 = 0;

const GRAB_BUSY_HELP: &str = "\
Couldn't grab the input device: it's already busy (Errno 16).
This almost always means one of two things:
  1. Wrong /dev/input/by-id entry. Gaming keyboards usually expose more than
     one event node for the same physical device - the real typing interface
     (ends in '-event-kbd') plus extra vendor/consumer-control interfaces
     (e.g. '-event-if01', '-event-if02', used for media keys, macros, RGB, etc).
     Run 'ls /dev/input/by-id/' and pass the one ending in '-event-kbd' to -k.
  2. Something else already grabbed this exact device - e.g. a previous run of
     this program that's still alive, or the chattering_fix.service if you've set
     it up. Check with 'systemctl status chattering_fix.service' and/or
     'pgrep -a keyboard-chatt' and stop the other instance before trying again.";

/// Grabs `device`, so that only we see its events, and relays them to a virtual
/// copy of it, leaving out key presses that look like chatter.
pub fn filter_chattering(mut device: Device, threshold_ms: u64) -> Result<Infallible> {
    // add delay to allow enter key to work after execution
    thread::sleep(Duration::from_secs(1));

    if let Err(e) = device.grab() {
        if e.raw_os_error() == Some(libc::EBUSY) {
            bail!(GRAB_BUSY_HELP);
        }
        return Err(e).context("Couldn't grab the input device");
    }

    // a copy of the device that we can write to - emits the filtered events to anyone who listens
    let mut virtual_device = create_virtual_twin(&device)?;
    let mut filter = ChatterFilter::new(threshold_ms);

    info!("Listening to input events...");

    loop {
        // blocks until there are events available
        for event in device.fetch_events().context("Couldn't read input events")? {
            if filter.should_forward(&event) {
                // `emit` terminates the batch with a SYN_REPORT
                virtual_device
                    .emit(&[event])
                    .context("Couldn't write to the virtual device")?;
            }
        }
    }
}

/// Creates a uinput device advertising the same identity and capabilities as `device`.
fn create_virtual_twin(device: &Device) -> Result<VirtualDevice> {
    let mut builder = VirtualDevice::builder()
        .context("Couldn't open /dev/uinput (are you running as root?)")?
        .name(device.name().unwrap_or("Chatter-filtered keyboard"))
        .input_id(device.input_id())
        .with_properties(device.properties())?;

    if let Some(keys) = device.supported_keys() {
        builder = builder.with_keys(keys)?;
    }
    if let Some(axes) = device.supported_relative_axes() {
        builder = builder.with_relative_axes(axes)?;
    }
    if device.supported_absolute_axes().is_some() {
        for (axis, info) in device.get_absinfo()? {
            builder = builder.with_absolute_axis(&UinputAbsSetup::new(axis, info))?;
        }
    }
    if let Some(switches) = device.supported_switches() {
        builder = builder.with_switches(switches)?;
    }
    if let Some(misc) = device.misc_properties() {
        builder = builder.with_msc(misc)?;
    }

    builder
        .build()
        .context("Couldn't create the virtual device")
}

/// Decides which events of a keyboard get relayed.
///
/// A key down event is considered chatter if it follows the key's previous up event
/// within the threshold. The key up event belonging to such a swallowed key down
/// event is swallowed as well.
pub struct ChatterFilter {
    threshold: Duration,
    /// Time of the last forwarded up event, per key.
    last_key_up: HashMap<KeyCode, Duration>,
    /// Keys whose down event has been forwarded without their up event following yet.
    key_pressed: HashSet<KeyCode>,
}

impl ChatterFilter {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            threshold: Duration::from_millis(threshold_ms),
            last_key_up: HashMap::new(),
            key_pressed: HashSet::new(),
        }
    }

    pub fn should_forward(&mut self, event: &InputEvent) -> bool {
        let event_type = event.event_type();

        // no need to relay those - we are going to emit our own
        if event_type == EventType::SYNCHRONIZATION || event_type == EventType::MISC {
            return false;
        }

        // some events we don't want to filter, like EV_LED for toggling NumLock
        // and the like, and also key hold events
        if event_type != EventType::KEY || event.value() > 1 {
            debug!("FORWARDING {}", describe(event));
            return true;
        }

        let now = event
            .timestamp()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        self.filter_key(KeyCode::new(event.code()), event.value(), now)
    }

    /// `value` is either 0 (up) or 1 (down); `now` is the event's timestamp.
    fn filter_key(&mut self, key: KeyCode, value: i32, now: Duration) -> bool {
        if value == KEY_UP {
            if self.key_pressed.remove(&key) {
                debug!("FORWARDING {key:?} up");
                self.last_key_up.insert(key, now);
                return true;
            }
            info!("FILTERING {key:?} up: key not pressed beforehand");
            return false;
        }

        if let Some(&prev) = self.last_key_up.get(&key) {
            let elapsed = now.saturating_sub(prev);
            if elapsed <= self.threshold {
                info!(
                    "FILTERED {key:?} down: last key up event happened {:.3} ms ago",
                    elapsed.as_secs_f64() * 1e3
                );
                return false;
            }
        }

        debug!("FORWARDING {key:?} down");
        self.key_pressed.insert(key);
        true
    }
}

fn describe(event: &InputEvent) -> String {
    if event.event_type() == EventType::KEY {
        format!("{:?}", KeyCode::new(event.code()))
    } else {
        format!("{:?} code {}", event.event_type(), event.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: KeyCode = KeyCode::KEY_A;
    const B: KeyCode = KeyCode::KEY_B;
    const DOWN: i32 = 1;
    const UP: i32 = 0;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn first_press_is_forwarded() {
        let mut f = ChatterFilter::new(30);
        assert!(f.filter_key(A, DOWN, ms(1000)));
    }

    #[test]
    fn bounce_within_threshold_is_filtered_together_with_its_release() {
        let mut f = ChatterFilter::new(30);
        assert!(f.filter_key(A, DOWN, ms(1000)));
        assert!(f.filter_key(A, UP, ms(1080)));
        // chatter: pressed again 10 ms after release
        assert!(!f.filter_key(A, DOWN, ms(1090)));
        // ... and its release has no matching forwarded press
        assert!(!f.filter_key(A, UP, ms(1095)));
    }

    #[test]
    fn filtered_bounce_does_not_extend_the_window() {
        // unlike X's Bounce Keys, swallowed events must not reset the delay
        let mut f = ChatterFilter::new(30);
        assert!(f.filter_key(A, DOWN, ms(1000)));
        assert!(f.filter_key(A, UP, ms(1100)));
        assert!(!f.filter_key(A, DOWN, ms(1110)));
        assert!(!f.filter_key(A, UP, ms(1115)));
        // 35 ms after the last *forwarded* release: a genuine press
        assert!(f.filter_key(A, DOWN, ms(1135)));
    }

    #[test]
    fn press_after_threshold_is_forwarded() {
        let mut f = ChatterFilter::new(30);
        assert!(f.filter_key(A, DOWN, ms(1000)));
        assert!(f.filter_key(A, UP, ms(1100)));
        assert!(f.filter_key(A, DOWN, ms(1131)));
    }

    #[test]
    fn threshold_itself_still_counts_as_chatter() {
        let mut f = ChatterFilter::new(30);
        assert!(f.filter_key(A, DOWN, ms(1000)));
        assert!(f.filter_key(A, UP, ms(1100)));
        assert!(!f.filter_key(A, DOWN, ms(1130)));
    }

    #[test]
    fn release_without_forwarded_press_is_filtered() {
        // e.g. the Enter key that was used to start the program
        let mut f = ChatterFilter::new(30);
        assert!(!f.filter_key(A, UP, ms(1000)));
    }

    #[test]
    fn keys_are_tracked_independently() {
        let mut f = ChatterFilter::new(30);
        assert!(f.filter_key(A, DOWN, ms(1000)));
        assert!(f.filter_key(A, UP, ms(1100)));
        assert!(f.filter_key(B, DOWN, ms(1105)));
        assert!(!f.filter_key(A, DOWN, ms(1110)));
    }

    #[test]
    fn clock_going_backwards_is_treated_as_chatter() {
        let mut f = ChatterFilter::new(30);
        assert!(f.filter_key(A, DOWN, ms(1000)));
        assert!(f.filter_key(A, UP, ms(1100)));
        assert!(!f.filter_key(A, DOWN, ms(1000)));
    }

    #[test]
    fn syn_and_msc_events_are_dropped_everything_else_non_key_passes() {
        let mut f = ChatterFilter::new(30);
        let syn = InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0);
        let msc = InputEvent::new(EventType::MISC.0, 4, 458756);
        let led = InputEvent::new(EventType::LED.0, 1, 1);
        let rel = InputEvent::new(EventType::RELATIVE.0, 0, 5);
        assert!(!f.should_forward(&syn));
        assert!(!f.should_forward(&msc));
        assert!(f.should_forward(&led));
        assert!(f.should_forward(&rel));
    }

    #[test]
    fn hold_events_pass_through() {
        let mut f = ChatterFilter::new(30);
        let hold = InputEvent::new(EventType::KEY.0, A.code(), 2);
        assert!(f.should_forward(&hold));
    }
}
