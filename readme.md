# Système de Monitoring Météo

Ce système se compose de deux parties principales :
1. Un client sur Raspberry Pi qui lit les données d'un capteur DHT11
2. Un serveur API qui stocke et expose les données

## 1. Installation Côté Serveur (VPS)

### Prérequis
```bash
# Mise à jour du système
sudo apt-get update && sudo apt-get upgrade

# Installation des dépendances
sudo apt-get install -y \
    pkg-config \
    libssl-dev \
    mariadb-server \
    nginx \
    monit
```

### Configuration de la Base de Données
```sql
# Connexion à MariaDB
sudo mysql -u root -p

# Création de la base et de l'utilisateur
CREATE DATABASE meteo_db;
CREATE USER '<db_name>'@'localhost' IDENTIFIED BY 'votre_mot_de_passe';
GRANT ALL PRIVILEGES ON meteo_db.* TO '<db_name>'@'localhost';
FLUSH PRIVILEGES;

# Création de la table
USE meteo_db;
CREATE TABLE measurements (
    id INT AUTO_INCREMENT PRIMARY KEY,
    temperature FLOAT NOT NULL,
    humidity FLOAT NOT NULL,
    timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    location VARCHAR(50) NOT NULL DEFAULT 'interior'
);
```

### Installation du Serveur API

1. Structure des répertoires :
```bash
sudo mkdir -p /opt/meteo-server
sudo useradd -r -s /bin/false meteo
```

2. Configuration :
```bash
# Fichier de configuration
sudo nano /opt/meteo-server/config.env
```
```env
DATABASE_URL=mysql://<db_name>:votre_mot_de_passe@localhost/meteo_db
PORT=<meteo_api_server_port>
RUST_LOG=info
```

3. Script de démarrage :
```bash
sudo nano /opt/meteo-server/start.sh
```
```bash
#!/bin/bash
mkdir -p /run/meteo-server
chown meteo:meteo /run/meteo-server
exec /opt/meteo-server/meteo_api_server
```
```bash
sudo chmod +x /opt/meteo-server/start.sh
```

4. Service Systemd :
```bash
sudo nano /etc/systemd/system/meteo-server.service
```
```ini
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
ExecStart=/opt/meteo-server/start.sh
Restart=always
RestartSec=5
StartLimitIntervalSec=0
PIDFile=/run/meteo-server/meteo-server.pid

[Install]
WantedBy=multi-user.target
```

5. Configuration Monit :
```bash
sudo nano /etc/monit/monitrc
```
```text
set httpd port 2812
    use address localhost
    allow localhost
    allow admin:votre_mot_de_passe
```

```bash
sudo nano /etc/monit/conf.d/meteo-server
```
```text
check process meteo-server with pidfile /run/meteo-server/meteo-server.pid
    start program = "/bin/systemctl start meteo-server"
    stop program = "/bin/systemctl stop meteo-server"
    
    if failed host localhost port <meteo_api_server_port> protocol http
        and request "/measurements"
        with timeout 10 seconds
        then restart
    
    if cpu usage > 95% for 10 cycles then alert
    if memory usage > 80% then restart
    if 5 restarts within 5 cycles then timeout
```

### Commandes Utiles Serveur
```bash
# Démarrer les services
sudo systemctl start mariadb
sudo systemctl start meteo-server
sudo systemctl start monit

# Activer au démarrage
sudo systemctl enable mariadb
sudo systemctl enable meteo-server
sudo systemctl enable monit

# Vérifier les statuts
sudo systemctl status meteo-server
sudo monit status

# Voir les logs
sudo journalctl -u meteo-server -f
```

## 2. Installation Côté Raspberry Pi

### Prérequis
```bash
# Mise à jour du système
sudo apt-get update && sudo apt-get upgrade

# Installation des dépendances
sudo apt-get install -y \
    pkg-config \
    libssl-dev
```

### Configuration DHT11
1. Branchement physique :
- VCC → 3.3V (Pin 1)
- DATA → GPIO4 (Pin 7)
- GND → Ground (Pin 6)
- Résistance pull-up 10kΩ entre VCC et DATA

2. Configuration du client :
```bash
sudo mkdir -p /opt/dht11
sudo nano /opt/dht11/config.env
```
```env
SERVER_URL=http://votre.serveur:<meteo_api_server_port>
SENSOR_LOCATION=interior
READ_INTERVAL_SECS=60
```

3. Service Systemd :
```bash
sudo nano /etc/systemd/system/dht11.service
```
```ini
[Unit]
Description=DHT11 Temperature and Humidity Monitor
After=network.target
StartLimitIntervalSec=0

[Service]
Type=simple
User=root
EnvironmentFile=/opt/dht11/config.env
WorkingDirectory=/opt/dht11
ExecStart=/opt/dht11/dht11_client
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### Commandes Utiles Raspberry Pi
```bash
# Démarrer le service
sudo systemctl start dht11

# Activer au démarrage
sudo systemctl enable dht11

# Vérifier le statut
sudo systemctl status dht11

# Voir les logs
sudo journalctl -u dht11 -f
```

## 3. API Endpoints

- `POST /push-measures` : Envoyer des mesures
- `GET /measurements` : Récupérer toutes les mesures
- `GET /measurements/{location}` : Récupérer les mesures par location
- `GET /stats` : Obtenir les statistiques

### Exemple de Requête
```bash
# Envoi de mesures
curl -X POST http://localhost:<meteo_api_server_port>/push-measures \
  -H "Content-Type: application/json" \
  -d '{
    "temperature": {"value": 23.5, "unit": "°C"},
    "humidity": {"value": 65.0, "unit": "%"},
    "location": "interior"
  }'
```

## 4. Maintenance

### Sauvegardes
```bash
# Sauvegarde de la base de données
mysqldump -u root -p meteo_db > backup.sql
```

### Surveillance
- Interface Monit : http://localhost:2812
- Logs système : `sudo journalctl -u meteo-server -f`
- Logs Nginx : `sudo tail -f /var/log/nginx/access.log`

### Redémarrage des Services
```bash
# Côté serveur
sudo systemctl restart meteo-server
sudo systemctl restart monit
sudo systemctl restart mariadb

# Côté Raspberry Pi
sudo systemctl restart dht11
```

## 5. Dépannage

1. Vérification des logs :
```bash
# Logs serveur
sudo journalctl -u meteo-server -f
# Logs client
sudo journalctl -u dht11 -f
```

2. Vérification des connexions :
```bash
# Test du port serveur
nc -zv localhost <meteo_api_server_port>
# Test de la base de données
mysql -u <db_name> -p meteo_db
```

3. Problèmes courants :
- Erreur "Permission denied" : Vérifier les permissions des répertoires
- Erreur de connexion : Vérifier la configuration réseau
- Erreur de lecture DHT11 : Vérifier le câblage et la résistance pull-up