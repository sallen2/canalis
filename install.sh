#!/usr/bin/env bash
# Installs Canalis for the current user on Arch Linux (Omarchy) or Fedora. A release download
# includes the canalis binary; in a source checkout it is built first.
set -euo pipefail

SOURCE_DIRECTORY="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$HOME/.local/bin/canalis"
ICON="$HOME/.local/share/icons/hicolor/256x256/apps/canalis.png"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
PREBUILT="$SOURCE_DIRECTORY/canalis"

# adb, ffmpeg and lm_sensors are packaged differently on each distribution.
install_packages() {
  case "$(distribution)" in
    arch) install_arch_packages ;;
    fedora) install_fedora_packages ;;
    *) require_programs ;;
  esac
}

# The distribution, or the one it is based on (ID and ID_LIKE in os-release): EndeavourOS counts
# as Arch and Nobara as Fedora. The package manager can't tell them apart, since Fedora packages
# pacman too, which then installs from Arch's repositories.
distribution() {
  local names name
  # shellcheck source=/dev/null
  names="$(. /etc/os-release 2> /dev/null && echo "${ID:-} ${ID_LIKE:-}")" || names=""
  for name in $names; do
    case "$name" in
      arch | fedora)
        echo "$name"
        return
        ;;
    esac
  done
  echo other
}

# Arch doesn't support partial upgrades. Installing from a repository that has moved on since the
# last `pacman -Syu` brings in packages built against newer libraries than the ones on disk;
# android-tools is the one that bites, because an adb linked to a protobuf that isn't installed
# cannot start, and Canalis needs adb for the screen's media, screenshots and theme pictures.
# Upgrading everything alongside the install keeps the system whole, and nothing runs at all when
# there is nothing to do. ffmpeg is what reads the screen's theme videos for the carousel.
install_arch_packages() {
  local packages=(android-tools ffmpeg lm_sensors)
  [[ -x "$PREBUILT" ]] || packages+=(rust)
  local missing=()
  local package
  for package in "${packages[@]}"; do
    pacman -Qq "$package" &> /dev/null || missing+=("$package")
  done
  if [[ ${#missing[@]} -eq 0 ]]; then
    echo "==> ${packages[*]} are already installed"
    return
  fi
  echo "==> Installing ${missing[*]}, with a full system upgrade so adb matches its libraries (sudo)"
  sudo pacman -Syu --needed "${missing[@]}"
}

# dnf brings along any newer library a package needs, so unlike on Arch nothing else has to be
# upgraded. Programs are looked for rather than packages: Fedora's ffmpeg-free and RPM Fusion's
# ffmpeg conflict, and adb or cargo may come from elsewhere (platform-tools, rustup).
install_fedora_packages() {
  local missing=()
  command -v adb &> /dev/null || missing+=(android-tools)
  command -v ffmpeg &> /dev/null || missing+=(ffmpeg-free)
  command -v sensors &> /dev/null || missing+=(lm_sensors)
  [[ -x "$PREBUILT" ]] || command -v cargo &> /dev/null || missing+=(cargo)
  if [[ ${#missing[@]} -eq 0 ]]; then
    echo "==> adb, ffmpeg and lm_sensors are already installed"
    return
  fi
  echo "==> Installing ${missing[*]} (sudo)"
  sudo dnf install "${missing[@]}"
}

# Other distributions name the packages differently, so only check that the programs are there.
require_programs() {
  local programs=(adb ffmpeg sensors)
  [[ -x "$PREBUILT" ]] || programs+=(cargo)
  local missing=()
  local program
  for program in "${programs[@]}"; do
    command -v "$program" &> /dev/null || missing+=("$program")
  done
  if [[ ${#missing[@]} -gt 0 ]]; then
    echo "==> Not found: ${missing[*]}. Install them with your package manager, then run this again" >&2
    echo "    (adb is packaged as adb or android-tools, and sensors as lm-sensors or lm_sensors)" >&2
    exit 1
  fi
  echo "==> ${programs[*]} are installed"
}

# Linux's kernel sensor drivers play LibreHardwareMonitor's role on Windows. CPU (k10temp,
# coretemp) and NVMe temperatures work on their own; the motherboard's Super I/O chip (fans,
# board temperature) needs lm_sensors to find and load its driver.
detect_hardware_sensors() {
  echo "==> Detecting motherboard sensors (sudo)"
  sudo sensors-detect --auto > /dev/null
  # sensors-detect saves the drivers it found for lm_sensors.service to load: in /etc/conf.d on
  # Arch, /etc/sysconfig on Fedora. Fedora has the service running already, so it is restarted to
  # load them now.
  if [[ -f /etc/conf.d/lm_sensors || -f /etc/sysconfig/lm_sensors ]]; then
    sudo systemctl enable lm_sensors.service
    sudo systemctl restart lm_sensors.service
  fi
  enable_msi_superio_driver
}

# MSI boards use a Nuvoton NCT6687D, whose kernel driver (nct6683) only loads with force=1.
enable_msi_superio_driver() {
  if ! grep -qi "micro-star" /sys/class/dmi/id/board_vendor 2> /dev/null || grep -qs "^nct6" /sys/class/hwmon/hwmon*/name; then
    return 0
  fi
  echo "==> Enabling the MSI motherboard sensor driver (nct6683 force=1)"
  echo "options nct6683 force=1" | sudo tee /etc/modprobe.d/canalis-nct6683.conf > /dev/null
  echo "nct6683" | sudo tee /etc/modules-load.d/canalis-nct6683.conf > /dev/null
  sudo modprobe nct6683 force=1 || echo "    (could not load nct6683 now; it will load after a reboot)"
}

build_and_install_binary() {
  if [[ -x "$PREBUILT" ]]; then
    echo "==> Using the included canalis binary"
    install -D -m 755 "$PREBUILT" "$BINARY"
    return
  fi
  echo "==> Building (release)"
  cargo build --release --locked --manifest-path "$SOURCE_DIRECTORY/Cargo.toml"
  install -D -m 755 "$SOURCE_DIRECTORY/target/release/canalis" "$BINARY"
}

install_udev_rule() {
  echo "==> Installing udev rule for the screen (sudo)"
  sudo install -m 644 "$SOURCE_DIRECTORY/packaging/70-tryx-panorama.rules" /etc/udev/rules.d/
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=tty --subsystem-match=usb
}

install_desktop_entry() {
  echo "==> Adding Canalis to the app launcher"
  install -D -m 644 "$SOURCE_DIRECTORY/packaging/canalis.desktop" "$HOME/.local/share/applications/canalis.desktop"
  "$BINARY" icon "$ICON" >/dev/null
}

install_service() {
  echo "==> Enabling the canalis user service"
  install -D -m 644 "$SOURCE_DIRECTORY/packaging/canalis.service" "$HOME/.config/systemd/user/canalis.service"
  systemctl --user daemon-reload
  systemctl --user enable --now canalis.service
  systemctl --user restart canalis.service
}

# Canalis was called Panorama before; an install under that name is replaced.
remove_old_panorama_install() {
  if systemctl --user is-enabled --quiet panorama.service 2> /dev/null || [[ -e "$HOME/.local/bin/panorama" ]]; then
    echo "==> Removing the old Panorama install (now Canalis)"
    systemctl --user disable --now panorama.service 2> /dev/null || true
    rm -f "$HOME/.config/systemd/user/panorama.service" "$HOME/.local/bin/panorama" \
      "$HOME/.local/share/applications/panorama.desktop" "$HOME/.local/share/icons/hicolor/256x256/apps/panorama.png"
    sudo rm -f /etc/modprobe.d/panorama-nct6683.conf /etc/modules-load.d/panorama-nct6683.conf
  fi
  if [[ -f "$CONFIG_HOME/panorama/settings.json" && ! -e "$CONFIG_HOME/canalis" ]]; then
    echo "==> Moving your settings to $CONFIG_HOME/canalis"
    mv "$CONFIG_HOME/panorama" "$CONFIG_HOME/canalis"
  fi
}

install_packages
remove_old_panorama_install
detect_hardware_sensors
build_and_install_binary
install_udev_rule
install_desktop_entry
install_service
echo
echo "Done. Open 'Canalis' from the launcher, or run: canalis"
echo "Service logs: journalctl --user -u canalis -f"
