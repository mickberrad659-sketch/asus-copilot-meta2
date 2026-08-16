#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
target_user=${SUDO_USER:-$USER}
target_home=$(getent passwd "$target_user" | cut -d: -f6)

sudo pacman -S --needed rust libxkbcommon acl
cargo build --release --manifest-path "$repo_dir/Cargo.toml"
install -Dm755 "$repo_dir/target/release/asus-copilot-meta2" "$target_home/.local/bin/asus-copilot-meta2"
install -Dm644 "$repo_dir/systemd/asus-copilot-meta2.service" "$target_home/.config/systemd/user/asus-copilot-meta2.service"

rule_tmp=$(mktemp)
trap 'rm -f "$rule_tmp"' EXIT
sed "s/@USER@/$target_user/g" "$repo_dir/udev/99-asus-copilot-meta2.rules.in" > "$rule_tmp"
sudo install -Dm644 "$rule_tmp" /etc/udev/rules.d/99-asus-copilot-meta2.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=input --action=add
sudo udevadm trigger --subsystem-match=misc --action=add

install -Dm755 "$repo_dir/scripts/build-niri-keymap" "$target_home/.local/bin/asus-copilot-meta2-keymap"
sudo -u "$target_user" ASUS_META2_LAYOUT="${ASUS_META2_LAYOUT:-us,ru}" ASUS_META2_OPTIONS="${ASUS_META2_OPTIONS:-grp:caps_toggle}" \
    "$target_home/.local/bin/asus-copilot-meta2-keymap" "$target_home/.config/niri/asus-meta2.xkb"

systemctl --user daemon-reload
systemctl --user enable --now asus-copilot-meta2.service
echo "Installed. For Niri, set keyboard.xkb.file to ~/.config/niri/asus-meta2.xkb"
