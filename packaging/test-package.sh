#!/bin/bash
# End-to-end check of the .deb, run as root on a disposable Ubuntu system
# (CI runner or container): after "apt remove", the users' GNOME settings must
# be restored, whether they are logged in or not.
#
# Usage: sudo packaging/test-package.sh path/to/clipboard-history.deb
set -euo pipefail

DEB=$(realpath "$1")
PATH_KEY=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clipboard-history/
MARKER=.config/clipboard-history/shortcut-installed

apt-get install -y -qq --no-install-recommends \
    gnome-settings-daemon-common gnome-shell-common \
    dbus dbus-user-session dconf-service dconf-gsettings-backend libglib2.0-bin >/dev/null
apt-get install -y -qq "$DEB" >/dev/null

# ---------------------------------------------------------------- helpers

fail() {
    echo "ÉCHEC : $*" >&2
    exit 1
}

# Runs a command in the user's D-Bus session (started for logged-in users).
in_session() {
    local user=$1
    shift
    local uid bus
    uid=$(id -u "$user")
    bus=/run/user/$uid/bus
    if [ -S "$bus" ]; then
        runuser -u "$user" -- env -i HOME="/home/$user" PATH=/usr/bin:/bin \
            XDG_RUNTIME_DIR="/run/user/$uid" DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" "$@"
    else
        runuser -u "$user" -- env -i HOME="/home/$user" PATH=/usr/bin:/bin \
            dbus-run-session -- "$@"
    fi
}

setting() {
    in_session "$1" gsettings get "$2" "$3"
}

# Creates a user and, if asked, simulates a login by starting a session bus.
create_user() {
    local user=$1 logged_in=$2
    useradd -m "$user"
    if [ "$logged_in" = yes ]; then
        local uid
        uid=$(id -u "$user")
        install -d -o "$user" -m 700 "/run/user/$uid"
        runuser -u "$user" -- dbus-daemon --session --fork \
            --address="unix:path=/run/user/$uid/bus"
    fi
}

expect_shortcut_installed() {
    local user=$1
    [[ $(setting "$user" org.gnome.settings-daemon.plugins.media-keys custom-keybindings) == *"$PATH_KEY"* ]] ||
        fail "$user : raccourci absent après install-shortcut"
    [[ $(setting "$user" org.gnome.shell.keybindings toggle-message-tray) != *"<Super>v"* ]] ||
        fail "$user : Super+V toujours pris par les notifications"
    [ -f "/home/$user/$MARKER" ] || fail "$user : marqueur absent"
}

expect_shortcut_removed() {
    local user=$1
    [[ $(setting "$user" org.gnome.settings-daemon.plugins.media-keys custom-keybindings) != *"$PATH_KEY"* ]] ||
        fail "$user : raccourci toujours présent après apt remove"
    [[ $(setting "$user" org.gnome.shell.keybindings toggle-message-tray) == *"<Super>v"* ]] ||
        fail "$user : Super+V non rendu aux notifications"
    [ ! -f "/home/$user/$MARKER" ] || fail "$user : marqueur toujours présent"
}

# ---------------------------------------------------------------- scenario

create_user logged-out no
create_user logged-in yes
create_user never-used no

for user in logged-out logged-in; do
    in_session "$user" clipboard-history install-shortcut >/dev/null
    expect_shortcut_installed "$user"
done

apt-get remove -y -qq clipboard-history >/dev/null
[ ! -e /usr/bin/clipboard-history ] || fail "programme toujours installé"

for user in logged-out logged-in; do
    expect_shortcut_removed "$user"
    echo "OK : $user"
done
[ ! -e /home/never-used/.config/clipboard-history ] || fail "never-used : fichiers créés"
echo "OK : never-used (non modifié)"
