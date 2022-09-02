//! Requires the "client", "standard_framework", and "voice" features be enabled in your
//! Cargo.toml, like so:
//!
//! ```toml
//! [dependencies.serenity]
//! git = "https://github.com/serenity-rs/serenity.git"
//! features = ["client", "standard_framework", "voice"]
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::{env, fs, vec};

use serenity::futures::lock::MutexGuard;
use serenity::futures::TryFutureExt;
use serenity::http::Http;
use serenity::model::id::ChannelId;
use serenity::prelude::{Mentionable, Mutex, TypeMapKey};
// This trait adds the `register_songbird` and `register_songbird_with` methods
// to the client builder below, making it easy to install this voice client.
// The voice client can be retrieved in any command using `songbird::get(ctx).await`.
use songbird::SerenityInit;

// Import the `Context` to handle commands.
use serenity::client::Context;

use serenity::{
    async_trait,
    client::{Client, EventHandler},
    framework::{
        standard::{
            macros::{command, group},
            Args, CommandResult,
        },
        StandardFramework,
    },
    model::{channel::Message, gateway::Ready},
    prelude::GatewayIntents,
    Result as SerenityResult,
};

use chrono::*;

use songbird::input::cached;
use songbird::tracks::TrackError;
use songbird::{
    driver::Bitrate,
    input::{
        self,
        cached::{Compressed, Memory},
        Input,
    },
    Call, Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent,
};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected at {}!", ready.user.name, Local::now());
    }
}

enum CachedSound {
    Compressed(Compressed),
    Uncompressed(Memory),
}

impl From<&CachedSound> for Input {
    fn from(obj: &CachedSound) -> Self {
        use CachedSound::*;
        match obj {
            Compressed(c) => c.new_handle().into(),
            Uncompressed(u) => u
                .new_handle()
                .try_into()
                .expect("Failed to create decoder for Memory source."),
        }
    }
}

struct SongMap;

impl TypeMapKey for SongMap {
    type Value = Arc<Mutex<HashMap<String, PathBuf>>>;
}

struct SongCache;

impl TypeMapKey for SongCache {
    type Value = Arc<Mutex<Vec<(String, Compressed)>>>;
}

struct TimeToKey;

impl TimeToKey {
    fn current_hour(&self) -> String {
        let hour = Local::now().hour();
        let mut key = String::new();
        key.push('0');
        if hour < 10 {
            key.push('0');
            key.push_str(hour.to_string().as_str());
        } else {
            key.push_str(hour.to_string().as_str());
        }
        key
    }

    fn next_hour(&self) -> String {
        let mut hour = Local::now().hour();
        if hour == 23 {
            hour = 0;
        } else {
            hour += 1;
        }
        let mut key = String::new();
        key.push('0');
        if hour < 10 {
            key.push('0');
            key.push_str(hour.to_string().as_str());
        } else {
            key.push_str(hour.to_string().as_str());
        }
        key
    }
}

async fn compress_song(file_path: &PathBuf) -> Compressed {
    let cached_song = Compressed::new(
        input::ffmpeg(file_path)
            .await
            .expect("File should be in songs folder."),
        Bitrate::BitsPerSecond(128_000),
    )
    .expect("These parameters are well-defined.");
    let _ = cached_song.raw.spawn_loader();
    cached_song
}

#[group]
#[commands(deafen, join, leave, mute, ping, undeafen, unmute, play)]
struct General;

//Todo: Consider making a config file to allow the changing of directory name.
const SONG_PATH: &str = "songs/";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let framework = StandardFramework::new()
        .configure(|c| c.prefix("~"))
        .group(&GENERAL_GROUP);

    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .register_songbird()
        .await
        .expect("Err creating client");

    {
        let mut data = client.data.write().await;

        let mut song_map = HashMap::new();

        for file in fs::read_dir(SONG_PATH).unwrap() {
            let file_path = file.unwrap().path();
            let file_path_str = file_path.display().to_string();
            let file_key = &file_path_str[SONG_PATH.chars().count()..SONG_PATH.chars().count() + 3];
            match file_key {
                "REA" => {}
                _ => {
                    song_map.insert(String::from(file_key), file_path);
                }
            }
        }

        println!("{:?}", song_map);
        println!("{} songs", song_map.len());

        let mut song_cache = vec![];

        let song_to_cache = TimeToKey.current_hour();

        let cached_path = song_map.get(&song_to_cache).unwrap();
        let cached_song = compress_song(cached_path).await;

        song_cache.push((song_to_cache, cached_song));

        //song_cache.push(compress_song(song_map.get(&songs_to_cache.1).unwrap()).await);

        println!("Amount of cached songs {}", song_cache.len());

        data.insert::<SongMap>(Arc::new(Mutex::new(song_map)));
        data.insert::<SongCache>(Arc::new(Mutex::new(song_cache)));
    }

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
}

#[command]
#[only_in(guilds)]
async fn play(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let (handler_lock, success_reader) = manager.join(guild_id, connect_to).await;

    let call_lock_for_evt = Arc::downgrade(&handler_lock);

    if let Ok(_reader) = success_reader {
        let mut handler = handler_lock.lock().await;
        check_msg(
            msg.channel_id
                .say(
                    &ctx.http,
                    &format!("Joined {} at {}", connect_to.mention(), Local::now()),
                )
                .await,
        );

        let key = TimeToKey.current_hour();

        let vec_sources_lock = ctx
            .data
            .read()
            .await
            .get::<SongCache>()
            .cloned()
            .expect("Sound cache was installed at startup.");
        let vec_sources_lock_for_evt = vec_sources_lock.clone();
        let mut vec_sources = vec_sources_lock.lock().await;
        let vec_source = vec_sources.remove(0);

        let hash_sources_lock = ctx
            .data
            .read()
            .await
            .get::<SongMap>()
            .cloned()
            .expect("Sound cache was installed at startup.");
        let hash_sources_lock_for_evt = hash_sources_lock.clone();
        let mut hash_sources = hash_sources_lock.lock().await;
        let hash_source = hash_sources;

        //Refactor and replace with match statement? May be edge case if the order somehow gets messed up. Look into ensuring this cannot happen
        if vec_source.0 != key {
            vec_sources.remove(0);
            if vec_sources.is_empty() {
                let this_hour_compressed = compress_song(hash_source.get(&key).unwrap()).await;
                vec_sources.push((key, this_hour_compressed));
            }
        }
        let source_clone = vec_source.1.clone();
        let song = handler.play_only_source(source_clone.into());
        let _ = song.set_volume(1.0);
        let _ = song.enable_loop();

        vec_sources.insert(0, vec_source);

        if vec_sources.len() == 1 {
            let next_hour_key = TimeToKey.next_hour();
            let next_hour_compressed =
                compress_song(hash_source.get(&next_hour_key).unwrap()).await;
            vec_sources.push((next_hour_key, next_hour_compressed));

            println!("cache contents: {:?}", vec_sources);
            println!("cache size: {:?}", vec_sources.len());
        }

        let chan_id = msg.channel_id;

        let send_http = ctx.http.clone();

        let now = Local::now();

        let next_hour = now.date().and_hms(now.hour() + 1, 0, 0);

        let time_to_top_hour = next_hour.signed_duration_since(now).to_std().unwrap();

        println!(
            "next hour: {} \ntime to next hour: {:?}",
            next_hour, time_to_top_hour
        );

        song.add_event(
            Event::Periodic(Duration::hours(1).to_std().unwrap(), Some(time_to_top_hour)),
            HourChange {
                chan_id,
                //handler: &handler,
                http: send_http,
            },
        );
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Error joining the channel")
                .await,
        );
    }

    Ok(())
}

struct HourChange {
    chan_id: ChannelId,
    //handler: MutexGuard<Call>,
    http: Arc<Http>,
}

#[async_trait]
impl VoiceEventHandler for HourChange {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        check_msg(
            self.chan_id
                .say(
                    &self.http,
                    &format!("It is now {} o' clock!", Local::now().hour()),
                )
                .await,
        );
        if let EventContext::Track(&[(_state, track)]) = ctx {
            let _ = track.disable_loop();
        }
        None
    }
}

#[command]
#[only_in(guilds)]
async fn deafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_deaf() {
        check_msg(msg.channel_id.say(&ctx.http, "Already deafened").await);
    } else {
        if let Err(e) = handler.deafen(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Deafened").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let _handler = manager.join(guild_id, connect_to).await;

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Left voice channel").await);
    } else {
        check_msg(msg.reply(ctx, "Not in a voice channel").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn mute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_mute() {
        check_msg(msg.channel_id.say(&ctx.http, "Already muted").await);
    } else {
        if let Err(e) = handler.mute(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Now muted").await);
    }

    Ok(())
}

#[command]
async fn ping(context: &Context, msg: &Message) -> CommandResult {
    check_msg(msg.channel_id.say(&context.http, "Pong!").await);

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn undeafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.deafen(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Undeafened").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to undeafen in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn unmute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.mute(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Unmuted").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to unmute in")
                .await,
        );
    }

    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
