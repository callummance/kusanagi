//! API calls and types for fetching a list of fights contained within a report as well
//! as related metadata
use crate::fflogs_api::api::{fflogs_request, ApiError, FFLogsApiClient};
use crate::fflogs_api::types::{Instance, Unit};

use http::uri::Uri;

use serde::{Deserialize, Serialize};

use log::info;

pub async fn request_fights(
    report_code: &str,
    translate: bool,
    api_client: &FFLogsApiClient,
) -> Result<ReportFightsList, ApiError> {
    info!(
        "Making API request to report.fights endpoint on report with code {}.",
        report_code
    );
    let url = construct_url(report_code, translate, api_client.api_key())?;
    let resp = api_client.run_request(url).await?;
    let res: ReportFightsList =
        serde_json::from_str(&resp).map_err(|err| ApiError::ResponseFormatError(err))?;
    return Ok(res);
}

pub fn construct_url(report_code: &str, translate: bool, api_key: &str) -> Result<Uri, ApiError> {
    let path = construct_path(report_code);
    let query = QueryParams {
        translate: Some(translate),
        api_key: api_key.to_owned(),
    };
    return fflogs_request(&path, query);
}

fn construct_path(report_code: &str) -> String {
    return format!("/v1/report/fights/{}", report_code);
}

#[derive(Serialize)]
struct QueryParams {
    translate: Option<bool>,
    api_key: String,
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct ReportFightsList {
    #[serde(rename = "fights")]
    pub fights: Vec<Fight>,
    #[serde(rename = "lang")]
    pub language: Option<String>,
    #[serde(rename = "friendlies")]
    pub friendlies: Vec<Unit>,
    #[serde(rename = "enemies")]
    pub enemies: Vec<Unit>,
    #[serde(rename = "friendlyPets")]
    pub friendly_pets: Vec<Unit>,
    #[serde(rename = "enemyPets")]
    pub enemy_pets: Vec<Unit>,
    #[serde(rename = "phases")]
    pub phases: Vec<Instance>,
    #[serde(rename = "logVersion")]
    pub log_version: Option<i32>,
    #[serde(rename = "title")]
    pub title: Option<String>,
    #[serde(rename = "owner")]
    pub owner: Option<String>,
    #[serde(rename = "start")]
    pub start: Option<u64>,
    #[serde(rename = "end")]
    pub end: Option<u64>,
    #[serde(rename = "zone")]
    pub zone: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Fight {
    #[serde(rename = "id")]
    id: i64,
    #[serde(rename = "start_time")]
    pub start_time: u64,
    #[serde(rename = "end_time")]
    pub end_time: u64,
    #[serde(rename = "boss")]
    pub boss: Option<i64>,
    #[serde(rename = "name")]
    pub name: Option<String>,
    #[serde(rename = "zoneID")]
    pub zone_id: Option<i64>,
    #[serde(rename = "zoneName")]
    pub zone_name: Option<String>,
    #[serde(rename = "size")]
    pub size: Option<i64>,
    #[serde(rename = "difficulty")]
    pub difficulty: Option<i64>,
    #[serde(rename = "kill")]
    pub kill: Option<bool>,
    #[serde(rename = "partial")]
    pub partial: Option<i64>,
    #[serde(rename = "standardComposition")]
    pub standard_composition: Option<bool>,
    #[serde(rename = "bossPercentage")]
    pub boss_percentage: Option<i32>,
    #[serde(rename = "fightPercentage")]
    pub fight_percentage: Option<i64>,
    #[serde(rename = "lastPhaseForPercentageDisplay")]
    pub last_phase_for_percentage_display: Option<i64>,
}
