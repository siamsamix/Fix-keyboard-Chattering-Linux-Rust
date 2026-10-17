# __Keyboard Chattering Fix for Linux__

_This is a rewrite of https://github.com/finkrer/KeyboardChatteringFix-Linux in Rust written using Claude._

__A tool for filtering mechanical keyboard chattering on Linux__


## Installation

You need a [Rust toolchain](https://rustup.rs). There is no dependency on `libevdev` or any other system library.

```shell
cargo build --release
sudo install -Dm755 target/release/keyboard-chattering-fix /usr/local/bin/keyboard-chattering-fix
```

## Usage

Reading `/dev/input/*` and writing to `/dev/uinput` requires root:

```shell
sudo keyboard-chattering-fix
```

### Customization Options

- -k KEYBOARD, --keyboard KEYBOARD
  - Name of your chattering keyboard device as listed in /dev/input/by-id. If left unset, will be attempted to be retrieved
  automatically. The device is captured `by-id`, and therefore in a persistent way.

- -t THRESHOLD, --threshold THRESHOLD
  - Filter time threshold in milliseconds. Default=30ms. Note: This does not denote the time between key presses, but
    between a key being
    released and pressed again, so the number should probably be lower than you might think. For reference, if you
    press the key really fast this delay is around 50 ms.

- -v {0,1,2}, --verbosity {0,1,2}
  - 0 is silent, 1 (default) logs filtered events, 2 logs all events.

## Automation

Starting the program manually every time doesn't sound like the greatest idea, so
you should probably consider something that does it for you.

Edit the `ExecStart` line of `chattering_fix.service`, so that it points to the installed binary and
contains your keyboard id and the desired threshold. For example:
```shell
ExecStart=/usr/local/bin/keyboard-chattering-fix -k usb-SINO_WEALTH_USB_KEYBOARD-event-kbd -t 50
```
Then, copy the `chattering_fix.service` to `/etc/systemd/system/` and enable it with the command below.
```shell
sudo systemctl enable --now chattering_fix
```
You can check if the systemd unit file is properly working using 
```shell
systemctl status chattering_fix.service
```
You can also use 
```shell
journalctl -xeu chattering_fix.service
```
just to make sure that there are no errors.
