//! Requires the "client", "standard_framework", and "voice" features be enabled in your
//! Cargo.toml, like so:
//!
//! ```toml
//! [dependencies.serenity]
//! git = "https://github.com/serenity-rs/serenity.git"
//! features = ["client", "standard_framework", "voice"]
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::{env, fs};

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
        println!(
            "{} is connected at {}!",
            ready.user.name,
            Local::now().to_string()
        );
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

struct SoundStore;

impl TypeMapKey for SoundStore {
    type Value = Arc<Mutex<HashMap<String, CachedSound>>>;
}
#[group]
#[commands(deafen, join, leave, mute, ping, undeafen, unmute, nook, play)]
struct General;

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

        let mut audio_map = HashMap::new();
        for file in fs::read_dir("songs/").unwrap() {
            let file_path = file.unwrap().path().display().to_string();
            match file_path.as_str() {
                "songs/README.txt" => {}
                _ => {
                    let cached_song = Memory::new(
                        input::ffmpeg(&file_path)
                            .await
                            .expect("File should be in root folder."),
                    )
                    .expect("These parameters are well-defined.");
                    let _ = cached_song.raw.spawn_loader();
                    let key = &file_path[6..9];
                    audio_map.insert(String::from(key), CachedSound::Uncompressed(cached_song));
                }
            }
        }

        println!("{:?}", audio_map.keys());
        println!("{} songs", audio_map.len());
        data.insert::<SoundStore>(Arc::new(Mutex::new(audio_map)));
    }

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
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

//Function for testing how to loop media playback from local storage
#[command]
#[only_in(guilds)]
async fn nook(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let source =
            match songbird::input::restartable::Restartable::ffmpeg("songs/020_8PM.flac", false)
                .await
            {
                Ok(source) => source,
                Err(why) => {
                    println!("Err starting source: {:?}", why);

                    check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg").await);

                    return Ok(());
                }
            };

        let song = handler.play_source(source.into());
        let _ = song.enable_loop();

        let local_time: DateTime<Local> = Local::now();
        let time_string = local_time.to_string();
        let mut play_message = String::from("Playing song at ");
        play_message.push_str(time_string.as_str());
        check_msg(msg.channel_id.say(&ctx.http, play_message).await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
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
                .say(&ctx.http, &format!("Joined {}", connect_to.mention()))
                .await,
        );

        let hour = Local::now().hour();
        let mut key = String::new();
        key.push('0');
        if hour < 10 {
            key.push('0');
            key.push_str(hour.to_string().as_str());
        } else{
            key.push_str(hour.to_string().as_str());
        }


        let sources_lock = ctx
            .data
            .read()
            .await
            .get::<SoundStore>()
            .cloned()
            .expect("Sound cache was installed at startup.");
        let sources_lock_for_evt = sources_lock.clone();
        let sources = sources_lock.lock().await;
        let source = sources.get(&key).expect("Handle placed into cache at startup.");

        let song = handler.play_source(source.into());
        let _ = song.set_volume(1.0);
        let _ = song.enable_loop();
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Error joining the channel")
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
