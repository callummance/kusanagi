use super::commands::report_stats::*;

use std::{collections::HashSet, sync::Arc};

use serenity::{
    async_trait,
    client::bridge::gateway::ShardManager,
    framework::standard::{
        macros::{group, hook},
        StandardFramework,
    },
    http::Http,
    model::{channel::Message, event::ResumedEvent, gateway::Ready, permissions::Permissions},
    prelude::*,
};

use crate::fight_analysis::analyse_fight::LogAnalysisClient;
use log::info;

pub struct ShardManagerContainer;
impl TypeMapKey for ShardManagerContainer {
    type Value = Arc<Mutex<ShardManager>>;
}
pub struct LogAnalysisClientContainer;
impl TypeMapKey for LogAnalysisClientContainer {
    type Value = LogAnalysisClient;
}

struct Handler;
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.tag());
    }

    async fn resume(&self, _: Context, _: ResumedEvent) {
        info!("Discord bot reconnected.")
    }
}

#[hook]
async fn before(ctx: &Context, msg: &Message, command_name: &str) -> bool {
    info!(
        "Got command '{}' from user '{}'",
        command_name, msg.author.name
    );
    let _ = msg.reply(ctx, "Warning: this bot is early pre-beta software. Please take anything it tells you with a mountain of salt.").await;
    true
}

#[group]
#[commands(fight_stats)]
struct FFLogs;

fn get_bot_permissions() -> Permissions {
    let mut perms = Permissions::empty();
    perms.set(Permissions::READ_MESSAGES, true);
    perms.set(Permissions::SEND_MESSAGES, true);
    return perms;
}

pub async fn start_bot(discord_token: String, analysis_client: LogAnalysisClient) {
    //Get bot owner
    let discord_http = Arc::new(Http::new_with_token(&discord_token));
    let (owners, bot_id) = match discord_http.get_current_application_info().await {
        Ok(info) => {
            let mut owners_set = HashSet::new();
            owners_set.insert(info.owner.id);
            (owners_set, info.id)
        }
        Err(why) => panic!("Unable to fetch discord client data due to {:?}", why),
    };

    //Configure client
    let framework = StandardFramework::new()
        .configure(|c| {
            c.with_whitespace(false)
                .on_mention(Some(bot_id))
                .prefix("?")
                .no_dm_prefix(true)
                .case_insensitivity(true)
                .owners(owners)
        })
        .before(before)
        .bucket("fflogs_api", |b| b.delay(5).time_span(60).limit(4))
        .await
        .group(&FFLOGS_GROUP);

    let mut discord_client = Client::new(&discord_token)
        .framework(framework)
        .event_handler(Handler)
        .await
        .expect("Error creating discord client");

    // Print invite link
    let current_user = discord_http.get_current_user().await;
    let perms = get_bot_permissions();
    let invite_url: String = match current_user {
        Ok(user) => user
            .invite_url(&discord_http, perms)
            .await
            .expect("Failed to fetch invite url"),
        Err(e) => panic!("Failed to fetch invite url due to error {:?}", e),
    };
    println!(
        "Use invite URL {} to add the bot to your server.",
        invite_url
    );

    //Insert fflogs client and shard manager
    {
        let mut data = discord_client.data.write().await;
        data.insert::<ShardManagerContainer>(Arc::clone(&discord_client.shard_manager));
        data.insert::<LogAnalysisClientContainer>(analysis_client);
    }

    if let Err(e) = discord_client.start_autosharded().await {
        panic!("Failed to start Discord bot due to reason {:?}", e);
    }
}
