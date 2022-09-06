extern crate chrono;
extern crate reqwest;
extern crate serde_json;

use chrono::*;
use reqwest::*;

pub const API_URL: &str = "https://api.openweathermap.org/data/2.5/";

#[derive(Debug, PartialEq)]
pub enum Weather {
    Clear,
    Rainy,
    Snowy,
    Unknown,
}

impl Weather {
    pub fn from_id(id: &str) -> Self {
        match id.chars().nth(0).unwrap_or_default() {
            '2' | '3' | '5' => Weather::Rainy,
            '6' => Weather::Snowy,
            '7' => Weather::Unknown, // TODO represents atmospheric conditions
            '8' => Weather::Clear,
            _ => Weather::Unknown,
        }
    }
}

pub struct Location {
    pub longitude: f64,
    pub latitude: f64,
}

pub struct WeatherData {
    pub last_call: DateTime<Utc>,
    pub cached_weather: Weather,
}

pub async fn get_weather(
    loc: &Location,
    api_key: &str,
    mut weather_cache: &WeatherData,
) -> Result<Weather> {
    let time_since_last_call = Utc::now().signed_duration_since(weather_cache.last_call);
    println!(
        "Time since last call to weather API: {}",
        time_since_last_call
    );
    if time_since_last_call < Duration::minutes(10) {
        println!("Calling weather API");
        weather_cache.last_call = Utc::now();
        let lat = loc.latitude;
        let lon = loc.longitude;
        let resp = reqwest::get(format!(
            "{}weather?lat={}&lon={}&appid={}",
            API_URL, lat, lon, api_key
        ))
        .await?
        .text()
        .await?;

        let json: serde_json::Value = match serde_json::from_str(&resp) {
            Ok(val) => val,
            Err(_) => serde_json::from_str("{}").unwrap(),
        };

        let weather_id = json
            .get("weather")
            .unwrap()
            .get(0)
            .unwrap()
            .get("id")
            .unwrap()
            .to_string();
        weather_cache.cached_weather = Weather::from_id(&weather_id);

        Ok(Weather::from_id(&weather_id))
    } else {
        Ok(WeatherData.cached_weather)
    }
}
