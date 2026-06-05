#!/bin/sh
# Post-install setup script for maranoded systemd service.
# Run as root after placing the binary at /usr/bin/maranoded.

set -e

# create dedicated user and group
if ! id -u maranode >/dev/null 2>&1; then
    useradd --system --no-create-home --shell /usr/sbin/nologin \
        --comment "Maranode AI runtime" maranode
fi

# data directory
install -d -o maranode -g maranode -m 0750 /var/lib/maranode
install -d -o maranode -g maranode -m 0750 /var/log/maranode

# install unit file
install -m 0644 maranoded.service /etc/systemd/system/maranoded.service

systemctl daemon-reload
systemctl enable maranoded

echo "Setup complete. Start the daemon with: systemctl start maranoded"
echo "View logs with: journalctl -u maranoded -f"
