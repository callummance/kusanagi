use dotenv;
use std::env;

use clap::{App, Arg};

use tokio::runtime::Runtime;

pub mod discord_bot;
pub mod fflogs_api;
pub mod fight_analysis;

const DEFAULT_PI_DIR: &'static str = "./phaseidentifiers";

pub fn start() {
    //Load config options
    let (discord_api_key, fflogs_api_key, definitions_dir) = load_options();
    //Create client for analysis
    let analysis_client =
        fight_analysis::analyse_fight::LogAnalysisClient::new(&fflogs_api_key, &definitions_dir)
            .unwrap();
    //Start bot
    let bot_future = discord_bot::start_bot::start_bot(discord_api_key, analysis_client);

    let _res = Runtime::new()
        .expect("Failed to create tokio runtime")
        .block_on(bot_future);
}

fn load_options() -> (String, String, String) {
    //Parse cli arguments
    let matches = App::new("Kusanagi discord bot")
        .version("0.2")
        .author("Kiiroi Yuki")
        .arg(
            Arg::with_name("discord_token_file")
                .short("d")
                .long("discord_tokfile")
                .value_name("FILE")
                .help("Path to file containing Discord API token")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("fflogs_token_file")
                .short("f")
                .long("fflogs_keyfile")
                .value_name("FILE")
                .help("Path to file containing FFLogs API token")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("phase_identifiers_dir")
                .short("p")
                .long("phase_identifiers")
                .value_name("PATH")
                .help("Path to directory containing phase definitions")
                .takes_value(true),
        )
        .get_matches();
    //Load environment variables
    dotenv::dotenv().ok();
    let discord_token: String = matches
        .value_of("discord_token_file")
        .map(|val| val.to_string())
        .or(env::var("discord_api_key_file").ok())
        .map(|path| std::fs::read_to_string(path).expect("Could not read Discord token file."))
        .or(env::var("discord_api_key").ok())
        .expect("Could not find Discord API key.");
    let fflogs_api_key: String = matches
        .value_of("fflogs_token_file")
        .map(|val| val.to_string())
        .or(env::var("fflogs_api_key_file").ok())
        .map(|path| std::fs::read_to_string(path).expect("Could not read fflogs API key file."))
        .or(env::var("fflogs_api_key").ok())
        .expect("Could not find FFLogs API key.");
    let phase_definitions_dir: String = matches
        .value_of("phase_identifiers_dir")
        .map(|val| val.to_string())
        .or(env::var("phaseidentifiers_dir").ok())
        .unwrap_or(DEFAULT_PI_DIR.to_string());

    return (discord_token, fflogs_api_key, phase_definitions_dir);
}
