# Razer Cooling Pad Linux

Control the Razer Laptop Cooling Pad (`1532:0f43`) directly from Linux with a temperature-driven fan curve.

This project talks to the pad over HID, so you can run custom curves without Razer Synapse. The HID protocol is based on the reverse-engineering work from [FanControl.RazerCoolingPadPlugin](https://github.com/Benson5650/FanControl.RazerCoolingPadPlugin).

## Features

- Configurable fan curves with linear interpolation between points
- Automatic thermal zone discovery from `/sys/class/thermal`
- Temperature hysteresis to reduce unnecessary fan step-downs
- Automatic reconnect after unplug/replug
- Clean shutdown that releases control back to the pad
- Included Arch package files, systemd service, and preset management

## Hardware

| Device | USB ID | Tested firmware |
| --- | --- | --- |
| Razer Laptop Cooling Pad | `1532:0f43` | `v1.10.00_r1` |

Fan speed range: `500` to `3200` RPM in `50` RPM steps.

## Install

### Arch Linux

```bash
sudo pacman -S --needed base-devel
makepkg -si
```

That installs:

- `razer-coolingpad` to `/usr/bin/razer-coolingpad`
- the service to `/usr/lib/systemd/system/razer-coolingpad.service`
- the udev rule to `/usr/lib/udev/rules.d/99-razer-coolingpad.rules`
- the default config to `/etc/razer-coolingpad-linux/config.json`
- preset curves to `/usr/share/razer-coolingpad-linux/curves/`

Enable the service after install:

```bash
sudo systemctl enable --now razer-coolingpad.service
```

### From source

```bash
git clone https://github.com/matthewyjiang/razer-cooling-pad-linux.git
cd razer-cooling-pad-linux
uv sync
```

Run commands in the project environment with `uv run`:

```bash
uv run razer-coolingpad sensors
sudo uv run razer-coolingpad
```

### System dependency

`hidapi` also needs the system library:

```bash
# Arch / Manjaro
sudo pacman -S hidapi

# Debian / Ubuntu
sudo apt install libhidapi-hidraw0

# Fedora
sudo dnf install hidapi
```

## Permissions

The Arch package installs the udev rule automatically. If the pad was already connected, unplug and replug it after installing.

For manual installs, either run the tool with `sudo` or install the included udev rule:

```bash
sudo install -Dm644 99-razer-coolingpad.rules /etc/udev/rules.d/99-razer-coolingpad.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

## Usage

### Common commands

```bash
# Run the fan curve service in the foreground
sudo razer-coolingpad

# List thermal zones
razer-coolingpad sensors

# Show current temperature, fan RPM, and active curve
razer-coolingpad status

# Run with the packaged balanced preset
sudo razer-coolingpad -c /usr/share/razer-coolingpad-linux/curves/balanced.json -v

# Set a fixed RPM
sudo razer-coolingpad set-rpm 2000

# Release control back to the pad
sudo razer-coolingpad off

# Generate a starter config
razer-coolingpad generate-config > my-curve.json

# Manage presets
razer-coolingpad preset list
razer-coolingpad preset current
razer-coolingpad preset status
sudo razer-coolingpad preset set balanced
```

### Global options

| Flag | Description |
| --- | --- |
| `-c`, `--config FILE` | Path to a JSON config |
| `--interval SEC` | Poll interval in seconds |
| `--sensor PATH` | Thermal sensor path; repeat to use more than one |
| `--hysteresis DEG` | Degrees C required before lowering fan speed |
| `-v`, `--verbose` | Print temperature and RPM each cycle |

### Subcommands

| Command | Description |
| --- | --- |
| `sensors` | Print detected thermal zones |
| `status` | Print current temperature, curve target, reported RPM, and active curve |
| `set-rpm RPM` | Set a fixed speed and exit |
| `off` | Release control and exit |
| `generate-config` | Print a sample JSON config |
| `preset list` | List available presets |
| `preset current` | Show the active preset |
| `preset status` | Show live status using the active preset config |
| `preset set NAME` | Copy a preset into the active config and restart the service if needed |

## Presets

Included presets live in `curves/` and are installed to `/usr/share/razer-coolingpad-linux/curves/`.

```bash
razer-coolingpad preset list
razer-coolingpad preset current
razer-coolingpad preset status
sudo razer-coolingpad preset set balanced
```

### `silent.json`

Quiet first. Fans stay off until `65C`.

| Temp | Fan % | Approx RPM |
| --- | --- | --- |
| `<=65C` | `0` | `500` |
| `75C` | `15` | `900` |
| `85C` | `40` | `1600` |
| `>=95C` | `70` | `2400` |

### `balanced.json`

General-purpose preset.

| Temp | Fan % | Approx RPM |
| --- | --- | --- |
| `<=55C` | `0` | `500` |
| `65C` | `25` | `1175` |
| `75C` | `55` | `1950` |
| `85C` | `80` | `2650` |
| `>=90C` | `100` | `3200` |

### `performance.json`

Starts earlier and ramps harder.

| Temp | Fan % | Approx RPM |
| --- | --- | --- |
| `<=45C` | `10` | `770` |
| `55C` | `35` | `1450` |
| `65C` | `60` | `2100` |
| `75C` | `85` | `2800` |
| `>=80C` | `100` | `3200` |

## Config format

```json
{
  "curve": [
    { "temp": 55, "percent": 0 },
    { "temp": 65, "percent": 25 },
    { "temp": 75, "percent": 55 },
    { "temp": 85, "percent": 80 },
    { "temp": 90, "percent": 100 }
  ],
  "interval": 3,
  "hysteresis": 2,
  "sensors": "auto"
}
```

| Field | Description |
| --- | --- |
| `curve` | List of `{temp, percent}` points used for linear interpolation |
| `interval` | Polling interval in seconds |
| `hysteresis` | Degrees C required before lowering fan speed |
| `sensors` | `"auto"` or an array of sysfs temperature paths |

## systemd service

The included service reads `/etc/razer-coolingpad-linux/config.json`.

```bash
sudo systemctl enable --now razer-coolingpad.service
sudo systemctl status razer-coolingpad.service
sudo journalctl -u razer-coolingpad.service -f
```

## Troubleshooting

### Device not found

- Check that `lsusb` shows `1532:0f43`
- Make sure you have root or the udev rule applied
- Close other software that may have claimed the device

### No temperatures available

- Run `razer-coolingpad sensors`
- Pass one or more explicit `--sensor` paths if auto-detection is not suitable

### Fans do not seem to change much

- The device only accepts `50` RPM increments
- Use `-v` to see the commanded RPM over time

## License

MIT
