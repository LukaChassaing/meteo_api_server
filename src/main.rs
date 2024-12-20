use std::fs::File;
use std::io::Write;
use std::process;
use actix_cors::Cors;
use actix_web::{post, get, web, App, HttpResponse, HttpServer, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{mysql::MySqlPoolOptions, Pool, MySql};
use std::env;
use dotenv::dotenv;
use thiserror::Error as ThisError;
use serde_json::json;
use std::error::Error;  // Ajout de cet import

async fn create_pid_file() -> Result<(), Box<dyn Error>> {
    let pid = process::id();
    let pid_path = "/run/meteo-server/meteo-server.pid";
    let mut file = File::create(pid_path)?;
    write!(file, "{}", pid)?;
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
enum Location {
    Interior,
    Exterior,
}

impl ToString for Location {
    fn to_string(&self) -> String {
        match self {
            Location::Interior => "interior".to_string(),
            Location::Exterior => "exterior".to_string(),
        }
    }
}

// Structures de données
#[derive(Serialize, Deserialize)]
struct Measurement {
    temperature: Temperature,
    humidity: Humidity,
    location: Location,
    timestamp: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize)]
struct Humidity {
    value: f32,
    unit: String,
}

#[derive(Serialize, Deserialize)]
struct Temperature {
    value: f32,
    unit: String,
}

// Structure pour les données retournées par la base de données
#[derive(Serialize)]
struct MeasurementRecord {
    temperature: f32,
    humidity: f32,
    location: String,
    timestamp: DateTime<Utc>,
}

// Structure pour les statistiques
#[derive(Serialize)]
struct LocationStats {
    location: String,
    current_temperature: Option<f32>,
    current_humidity: Option<f32>,
    avg_temperature_24h: Option<f32>,
    avg_humidity_24h: Option<f32>,
    min_temperature_24h: Option<f32>,
    max_temperature_24h: Option<f32>,
}

// Gestion des erreurs
#[derive(ThisError, Debug)]
enum ApiError {
    #[error("Erreur de base de données: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Erreur de configuration: {0}")]
    ConfigError(String),
}

impl actix_web::error::ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError().json(json!({
            "error": self.to_string()
        }))
    }
}

// Structure pour la configuration de l'application
struct AppState {
    db: Pool<MySql>,
}

// Routes
#[post("/push-measures")]
async fn push_meteo_data(
    data: web::Json<Measurement>,
    state: web::Data<AppState>,
) -> Result<impl Responder, ApiError> {
    let mut measurement = data.into_inner();
    measurement.timestamp = Some(Utc::now());

    sqlx::query!(
        r#"
        INSERT INTO measurements (temperature, humidity, timestamp, location)
        VALUES (?, ?, ?, ?)
        "#,
        measurement.temperature.value,
        measurement.humidity.value,
        measurement.timestamp,
        measurement.location.to_string(),
    )
    .execute(&state.db)
    .await?;

    Ok(HttpResponse::Ok().json(measurement))
}

#[get("/measurements")]
async fn get_measurements(state: web::Data<AppState>) -> Result<impl Responder, ApiError> {
    let records = sqlx::query_as!(
        MeasurementRecord,
        r#"
        SELECT temperature, humidity, timestamp, location
        FROM measurements
        WHERE timestamp >= DATE_SUB(NOW(), INTERVAL 30 DAY)
        ORDER BY timestamp ASC
        "#
    )
    .fetch_all(&state.db)
    .await?;

    Ok(HttpResponse::Ok().json(records))
}

#[get("/measurements/{location}")]
async fn get_measurements_by_location(
    location: web::Path<String>,
    state: web::Data<AppState>,
) -> Result<impl Responder, ApiError> {
    let records = sqlx::query_as!(
        MeasurementRecord,
        r#"
        SELECT temperature, humidity, timestamp, location
        FROM measurements
        WHERE location = ?
        AND timestamp >= DATE_SUB(NOW(), INTERVAL 30 DAY)
        ORDER BY timestamp ASC
        "#,
        location.to_string(),
    )
    .fetch_all(&state.db)
    .await?;

    Ok(HttpResponse::Ok().json(records))
}

#[get("/stats")]
async fn get_stats(state: web::Data<AppState>) -> Result<impl Responder, ApiError> {
    let stats = sqlx::query_as!(
        LocationStats,
        r#"
        WITH current_measures AS (
            SELECT 
                location,
                CAST(temperature AS FLOAT) as current_temperature,
                CAST(humidity AS FLOAT) as current_humidity
            FROM measurements
            WHERE (location, timestamp) IN (
                SELECT location, MAX(timestamp)
                FROM measurements
                GROUP BY location
            )
        ),
        daily_stats AS (
            SELECT 
                location,
                CAST(AVG(temperature) AS FLOAT) as avg_temperature_24h,
                CAST(AVG(humidity) AS FLOAT) as avg_humidity_24h,
                CAST(MIN(temperature) AS FLOAT) as min_temperature_24h,
                CAST(MAX(temperature) AS FLOAT) as max_temperature_24h
            FROM measurements
            WHERE timestamp >= NOW() - INTERVAL 30 DAY
            GROUP BY location
        )
        SELECT 
            cm.location,
            cm.current_temperature,
            cm.current_humidity,
            ds.avg_temperature_24h,
            ds.avg_humidity_24h,
            ds.min_temperature_24h,
            ds.max_temperature_24h
        FROM current_measures cm
        JOIN daily_stats ds ON cm.location = ds.location
        "#
    )
    .fetch_all(&state.db)
    .await?;

    // Filtrer les valeurs NULL si nécessaire et formater la réponse
    let formatted_stats: Vec<_> = stats.into_iter()
        .map(|stat| serde_json::json!({
            "location": stat.location,
            "current": {
                "temperature": stat.current_temperature,
                "humidity": stat.current_humidity
            },
            "daily": {
                "average": {
                    "temperature": stat.avg_temperature_24h,
                    "humidity": stat.avg_humidity_24h
                },
                "min_temperature": stat.min_temperature_24h,
                "max_temperature": stat.max_temperature_24h
            }
        }))
        .collect();

    Ok(HttpResponse::Ok().json(formatted_stats))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Création du fichier PID
    if let Err(e) = create_pid_file().await {
        eprintln!("Erreur lors de la création du fichier PID: {}", e);
        return Ok(());
    }


    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let port = env::var("PORT")
        .unwrap_or_else(|_| "4350".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid number");

        let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool");

    println!("Starting server at port {}", port);

    let server = HttpServer::new(move || {
        let cors = Cors::permissive();
        
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
            }))
            .wrap(cors)
            .wrap(actix_web::middleware::Logger::default())
            .service(push_meteo_data)
            .service(get_measurements)
            .service(get_measurements_by_location)
            .service(get_stats)
    })
    .bind(("0.0.0.0", port))?;

    println!("Server running at http://0.0.0.0:{}", port);
    
    // Supprimer le fichier PID à la fin
    let pid_path = "/run/meteo-server/meteo-server.pid".to_string();
    ctrlc::set_handler(move || {
        if let Err(e) = std::fs::remove_file(&pid_path) {
            eprintln!("Erreur lors de la suppression du fichier PID: {}", e);
        }
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");

    server.run().await
}