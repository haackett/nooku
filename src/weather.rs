extern crate reqwest;

use std::fmt;
use reqwest::*;

pub const API_URL:&str = "https://api.openweathermap.org/data/2.5/";

#[derive(Debug)]
pub enum Weather {
    Clear,
    Rainy,
    Snowy,
    Unknown,
}

impl Weather {
    pub fn from_str(s: &str) -> Self {
        match s {
            "Clear" => Weather::Clear,
            "Raining" => Weather::Rainy,
            "Snowing" => Weather::Snowy,
            _ => Weather::Unknown,
        }

    }
}

pub struct Location {
    pub longitude: f64,
    pub latitude: f64,
}


pub async fn get_weather(loc: Location, api_key: &str) -> Result<Weather, > {
    let lat = loc.latitude;
    let lon = loc.longitude;
    let resp = reqwest::get(format!("{}weather?lat={}&lon={}&appid={}",API_URL,lat,lon,api_key))
        .await?
        .text()
        .await?;

    let json: serde_json::Value = match serde_json::from_str(&resp){
        Ok(val) => val,
        Err(_) => serde_json::from_str("{}").unwrap(), 
    };

    println!("{:#?}", json);

    let weather_string: String = json.get("weather").unwrap()
        .get(0).unwrap()
        .get("main").unwrap().to_string();

    println!("{}", weather_string);

    Ok(Weather::from_str(&weather_string))
}
