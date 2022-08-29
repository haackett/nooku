extern crate reqwest;

use std::fmt;


use reqwest::*;

pub const API_URL:&str = "https://api.openweathermap.org/data/2.5/";

#[derive(Debug)]
pub enum Weather {
    Clear,
    Rainy,
    Snowy,
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
    println!("{:#?}", resp);
    Ok(Weather::Snowy)
}
