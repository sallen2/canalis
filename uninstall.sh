#!/usr/bin/env bash
# Removes Canalis. Your settings in ~/.config/canalis are kept, and so are lm_sensors and adb.
set -euo pipefail

systemctl --user disable --now canalis.service 2>/dev/null || true
rm -f "$HOME/.config/systemd/user/canalis.service"
systemctl --user daemon-reload
rm -f "$HOME/.local/bin/canalis" "$HOME/.local/share/applications/canalis.desktop" "$HOME/.local/share/icons/hicolor/256x256/apps/canalis.png"
sudo rm -f /etc/udev/rules.d/70-tryx-panorama.rules
sudo udevadm control --reload
sudo rm -f /etc/modprobe.d/canalis-nct6683.conf /etc/modules-load.d/canalis-nct6683.conf
echo "Removed. Settings kept in ${XDG_CONFIG_HOME:-$HOME/.config}/canalis/"
