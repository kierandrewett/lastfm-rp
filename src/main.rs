use discord_rich_presence::activity::Timestamps;
use dotenv::dotenv;
use lastfm::{imageset::ImageSet, Client};
use std::{error::Error, time::Instant};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use discord_rich_presence::{activity::{Activity, Assets, Button}, DiscordIpc, DiscordIpcClient};

fn create_lastfm_client() -> Result<Client<String, String>, Box<dyn Error>> {
    let api_key = std::env::var("LASTFM_API_KEY")
        .expect("Missing LASTFM_API_KEY env variable");

    let username = std::env::var("LASTFM_USERNAME")
        .expect("Missing LASTFM_USERNAME env variable");

    let client = Client::builder()
        .api_key(api_key)
        .username(username)
        .build();

    Ok(client)
}

fn create_discord_client() -> Result<DiscordIpcClient, Box<dyn Error>> {
    let client_id = std::env::var("DISCORD_CLIENT_ID")
        .expect("Missing DISCORD_CLIENT_ID env variable");

    let mut discord = DiscordIpcClient::new(&client_id)
        .expect("Failed to create Discord RPC client");

    discord.connect()
        .expect("Failed to connect to Discord RPC client");

    Ok(discord)
}

fn get_lastfm_art_or_fallback(album_art: ImageSet) -> String {
    let fallback_album_art = "blank_art".to_string();

    let album_art_url = album_art.large
        .or_else(|| album_art.medium.clone())
        .or_else(|| album_art.small.clone())
        .unwrap_or(fallback_album_art.clone());

    if album_art_url.contains("2a96cbd8b46e442fc41c2b86b821562f") {
        fallback_album_art
    } else {
        album_art_url
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Scrobble {
    track_name: String,
    artist_name: String,
    album_art_url: String,
    track_url: String,
}

struct Application {
    lastfm: Client<String, String>,
    discord: DiscordIpcClient,

    current_track: Option<Scrobble>,
    current_track_started: SystemTime,

    timer_active: bool,
    timer_started: Instant,
}
impl Application {
    async fn process_loop(&mut self) {
        loop {
            self.update_current_activity().await;

            if !self.timer_active || ((Instant::now() - self.timer_started) > Duration::from_secs(20)) {
                self.timer_active = true;
                self.timer_started = Instant::now();

                if let Some(track) = &self.current_track {
                    let state = track.artist_name.clone();
                    let details = track.track_name.clone();
                    let status_text = format!("{details} by {state}");
                    let assets = Assets::new()
                        .large_image(&track.album_art_url)
                        .large_text(&status_text)
                        .small_image("lastfm")
                        .small_text("Last.fm");
                    let track_started = self.current_track_started.duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
                    let timestamps = Timestamps::new()
                        .start(track_started);
                    let buttons = vec![
                        Button::new("Listen on Last.fm", &track.track_url)
                    ];

                    println!("Discord: Updating activity with:\n    Details: {details}\n    State: {state}\n    Image: {}", track.album_art_url);

                    let activity = Activity::new()
                        .details(&details)
                        .state(&state)
                        .assets(assets)
                        .timestamps(timestamps)
                        .buttons(buttons);
    
                    self.discord.set_activity(activity).unwrap();
                } else {
                    println!("Discord: Playback stopped, clearing activity.");
    
                    self.discord.clear_activity().unwrap();
                }
            }

            std::thread::sleep(Duration::from_millis(2000));

        }
    }

    async fn update_current_activity(&mut self) {
        match self.lastfm.now_playing().await {
            Ok(track_opt) => {
                if let Some(track) = track_opt {
                    let new_track = Scrobble {
                        track_name: track.name,
                        artist_name: track.artist.name,
                        album_art_url: get_lastfm_art_or_fallback(track.image),
                        track_url: track.url
                    };

                    // Check if the current track is the same as the new one
                    if self.current_track.is_none() || self.current_track.clone().unwrap() != new_track {
                        println!("Last.fm: Updating track information to:\n    Track: {}\n    Artist: {}.", new_track.track_name, new_track.artist_name);

                        self.timer_active = false;
                        self.current_track = Some(new_track);
                        self.current_track_started = SystemTime::now();
                    }
                } else {
                    self.current_track = None;
                }
            },
            Err(err) => {
                eprintln!("Failed to update current activity: {}", err);
            }
        }
    }

    fn new() -> Application {
        let lastfm = create_lastfm_client()
            .expect("Failed to create Last.fm client");

        let discord = create_discord_client()
            .expect("Failed to create Discord RPC client");

        Application {
            lastfm,
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
