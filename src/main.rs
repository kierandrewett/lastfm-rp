use chrono::{DateTime, TimeZone, Utc};
use discord_rich_presence::activity::Timestamps;
use dotenv::dotenv;
use uuid::Uuid;
use std::{error::Error, time::Instant};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use discord_rich_presence::{activity::{Activity, Assets, Button}, DiscordIpc, DiscordIpcClient};
use serde_json::{json, Value};
use reqwest::Url;
use serde::{Deserialize, Deserializer, Serialize};
use serde::de::Error as _LFMError;

fn create_discord_client() -> Result<DiscordIpcClient, Box<dyn Error>> {
    let client_id = std::env::var("DISCORD_CLIENT_ID")
        .expect("Missing DISCORD_CLIENT_ID env variable");

    let mut discord = DiscordIpcClient::new(&client_id)
        .expect("Failed to create Discord RPC client");

    discord.connect()
        .expect("Failed to connect to Discord RPC client");

    Ok(discord)
}

fn str_bool_to_real_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Deserialize::deserialize(deserializer)?;

    return if let Some(str) = s {
        Ok(match str.to_lowercase().as_str() {
            "1" | "true" => true,
            _ => false
        })
    } else {
        Ok(false)
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct TrackImage {
    size: String,
    #[serde(rename = "#text")]
    url: String,
}

fn deserialize_images<'de, D>(deserializer: D) -> Result<LFMImageSet, D::Error>
where
    D: Deserializer<'de>,
{
    let images: Vec<TrackImage> = Deserialize::deserialize(deserializer)?;
    let mut track_image_set = LFMImageSet {
        small: None,
        medium: None,
        large: None,
        extralarge: None,
    };

    for image in images {
        let image_url = if image.url.contains("2a96cbd8b46e442fc41c2b86b821562f") {
            None
        } else {
            Some(image.url)
        };

        match image.size.as_str() {
            "small" => track_image_set.small = image_url,
            "medium" => track_image_set.medium = image_url,
            "large" => track_image_set.large = image_url,
            "extralarge" => track_image_set.extralarge = image_url,
            _ => {},
        }
    }

    Ok(track_image_set)
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct TrackAlbum {
    #[serde(rename = "#text")]
    name: String,
    mbid: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct TrackArtist {
    mbid: String,

    name: String,
    url: String,

    #[serde(deserialize_with = "deserialize_images")]
    image: LFMImageSet
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct LFMImageSet {
    small: Option<String>,
    medium: Option<String>,
    large: Option<String>,
    extralarge: Option<String>,
}

impl LFMImageSet {
    fn to_vec(&self) -> Vec<&str> {
        let mut images = Vec::new();

        if let Some(small) = &self.small {
            images.push(small.as_str());
        }

        if let Some(medium) = &self.medium {
            images.push(medium.as_str());
        }

        if let Some(large) = &self.large {
            images.push(large.as_str());
        }

        if let Some(extralarge) = &self.extralarge {
            images.push(extralarge.as_str());
        }

        images
    }
}

fn unix_to_datetime<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let str = String::deserialize(deserializer)?;

    let timestamp_seconds: i64 = str.parse().map_err(D::Error::custom)?;

    // Convert the Unix timestamp to DateTime<Utc>
    let datetime = Utc.timestamp_opt(timestamp_seconds, 0)
        .single()
        .ok_or_else(|| D::Error::custom("Invalid timestamp"))?;

    Ok(datetime)
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct TrackAttr {
    #[serde(default, deserialize_with = "str_bool_to_real_bool", rename = "nowplaying")]
    now_playing: bool,
}

fn attr_now_playing<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let attr = Value::deserialize(deserializer)?;

    Ok(attr
        .as_object()
        .and_then(|obj| obj.get("nowplaying"))
        .and_then(|val| val.as_str())
        .unwrap_or("false") == "true")
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Track {
    #[serde(rename = "@attr", default, deserialize_with = "attr_now_playing")]
    now_playing: bool,

    #[serde(default, deserialize_with = "str_bool_to_real_bool")]
    streamable: bool,

    mbid: String,

    name: String,
    url: String,

    #[serde(rename = "date.uts", default, deserialize_with = "unix_to_datetime")]
    date: DateTime<Utc>,

    artist: TrackArtist,
    album: TrackAlbum,

    #[serde(deserialize_with = "deserialize_images")]
    image: LFMImageSet
}

struct Application {
    discord: DiscordIpcClient,

    current_track: Option<Track>,
    current_track_started: SystemTime,

    timer_active: bool,
    timer_started: Instant,
}
impl Application {
    async fn process_loop(&mut self) {
        loop {
            match self.update_current_activity().await {
                Ok(_) => {},
                Err(e) => {
                    eprintln!("Last.fm: {}", e);
                }
            }

            if !self.timer_active || ((Instant::now() - self.timer_started) > Duration::from_secs(20)) {
                self.timer_active = true;
                self.timer_started = Instant::now();

                if let Some(track) = &self.current_track {
                    let state = format!("by {}", track.artist.name.clone());
                    let details = track.name.clone();
                    let status_text = format!("on {}", track.album.name.clone());

                    let album_art = track.image.to_vec();
                    let album_art_url = album_art.last().unwrap_or(&"blank_art");

                    let assets = Assets::new()
                        .large_image(album_art_url)
                        .large_text(&status_text)
                        .small_image("lastfm")
                        .small_text("Last.fm");

                    let track_started = self.current_track_started.duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
                    let timestamps = Timestamps::new()
                        .start(track_started);
                    let buttons = vec![
                        Button::new("Listen on Last.fm", &track.url)
                    ];

                    println!("Discord: Updating activity with:\n{:#?}", track);

                    let activity = Activity::new()
                        .details(&details)
                        .state(&state)
                        .assets(assets)
                        .timestamps(timestamps)
                        .buttons(buttons);

                    let mut activity_json = serde_json::to_value(&activity)
                        .unwrap();

                    let activity_json_mut = activity_json
                        .as_object_mut()
                        .unwrap();

                    activity_json_mut.insert("type".into(), 2.into());

                    let data = json!({
                        "cmd": "SET_ACTIVITY",
                        "args": {
                            "pid": std::process::id(),
                            "activity": activity_json
                        },
                        "nonce": Uuid::new_v4().to_string()
                    });

                    self.discord.send(data, 1).unwrap();
                } else {
                    println!("Discord: Playback stopped, clearing activity.");

                    self.discord.clear_activity().unwrap();
                }
            }

            std::thread::sleep(Duration::from_millis(2000));

        }
    }

    async fn update_current_activity(&mut self) -> Result<(), Box<dyn Error>>  {
        let api_key = std::env::var("LASTFM_API_KEY")
            .expect("Missing LASTFM_API_KEY env variable");

        let username = std::env::var("LASTFM_USERNAME")
            .expect("Missing LASTFM_USERNAME env variable");

        let url_query = vec![
            ("method", "user.getrecenttracks".to_string()),
            ("user", username),
            ("format", "json".to_string()),
            ("extended", "1".to_string()),
            ("api_key", api_key.to_string()),
            ("limit", "1".to_string()),
        ];

        let url = Url::parse_with_params("https://ws.audioscrobbler.com/2.0/", &url_query)
            .unwrap();

        let body = reqwest::get(url)
            .await
            .expect("Failed to contact Last.fm.")
            .text()
            .await
            .expect("Failed get Last.fm response.");

        let json: Value = serde_json::from_str(&body)
            .expect("Failed to cast Last.fm response as JSON.");

        let latest_tracks = json
            .as_object()
            .ok_or("Failed to cast response as JSON object.")?
            .get("recenttracks")
            .ok_or("Failed to get recenttracks key.")?
            .get("track")
            .ok_or("Failed to get track key.")?
            .as_array()
            .ok_or("Failed to cast track key as array.")?;

        let latest_tracks: Vec<Track> = serde_json::from_value(Value::Array(latest_tracks.clone()))?;

        let now_playing = latest_tracks
            .iter()
            .find(|x|  x.now_playing);

        match now_playing {
            Some(track) => {
                // Check if the current track is the same as the new one
                if self.current_track.is_none() || &self.current_track.clone().unwrap() != track {
                    println!("Last.fm: Updating track information to: {:#?}", track);

                    self.timer_active = false;
                    self.current_track = Some(track.clone());
                    self.current_track_started = SystemTime::now();
                }
            },
            _ => {
                self.current_track = None;
            }
        }

        Ok(())
    }

    fn new() -> Application {
        let discord = create_discord_client()
            .expect("Failed to create Discord RPC client");

        Application {
            discord,

            current_track: None,
            current_track_started: SystemTime::now(),

            timer_active: false,
            timer_started: Instant::now()
        }
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok().expect("Failed to load .env");

    let mut app = Application::new();

    app.process_loop().await;

    app.discord.close().unwrap();
}
