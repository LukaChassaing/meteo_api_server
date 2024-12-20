#!/bin/bash

# Vérifie si on est en root
if [ "$EUID" -ne 0 ]; then 
    echo "Ce script doit être exécuté en tant que root"
    exit 1
fi

# Crée le répertoire d'installation
mkdir -p /opt/meteo-server

# Copie l'exécutable
cp ./target/release/meteo_api_server /opt/meteo-server/

# Crée le fichier de configuration
cat > /opt/meteo-server/config.env << EOL
DATABASE_URL=mysql://<db_name>:<sql_username>@localhost/meteo_db
PORT=<meteo_api_server_port>
RUST_LOG=info
EOL

# Ajuste les permissions
chown -R meteo:meteo /opt/meteo-server
chmod 755 /opt/meteo-server
chmod 640 /opt/meteo-server/config.env
chmod 755 /opt/meteo-server/meteo_api_server

# Crée un utilisateur système si nécessaire
if ! id "meteo" &>/dev/null; then
    useradd -r -s /bin/false meteo
fi

# Copie et active le service systemd
cat > /etc/systemd/system/meteo-server.service << EOL
[Unit]
Description=Meteo API Server
After=network.target mysql.service
Requires=mysql.service

[Service]
Type=simple
User=meteo
Group=meteo
EnvironmentFile=/opt/meteo-server/config.env
WorkingDirectory=/opt/meteo-server
ExecStart=/opt/meteo-server/meteo_api_server
Restart=always
RestartSec=5
StartLimitIntervalSec=0

# Sécurité
NoNewPrivileges=yes
ProtectSystem=full
ProtectHome=yes
PrivateTmp=yes
ProtectKernelTunables=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes

[Install]
WantedBy=multi-user.target
EOL

# Configure le logrotate
cat > /etc/logrotate.d/meteo-server << EOL
/var/log/meteo-server/*.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
    create 640 meteo meteo
}
EOL

# Crée le répertoire pour les logs
mkdir -p /var/log/meteo-server
chown -R meteo:meteo /var/log/meteo-server
chmod 755 /var/log/meteo-server

# Active et démarre le service
systemctl daemon-reload
systemctl enable meteo-server
systemctl start meteo-server

echo "Installation terminée. Vérifiez le statut avec:"
echo "systemctl status meteo-server"