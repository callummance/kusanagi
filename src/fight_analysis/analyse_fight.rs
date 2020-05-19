use super::phase_definition::{
    load_definitions_files, DefinitionsLoadError, PhaseDefinitionsCollection, PhaseDefinitionsPhase,
};
use crate::fflogs_api::api::{new_fflogs_api_client, ApiError, FFLogsApiClient};
use crate::fflogs_api::report::events::ReportEvent;
use crate::fflogs_api::report::fights::{request_fights, Fight, ReportFightsList};

use lazy_static::*;
use regex::Regex;

use chrono::{DateTime, LocalResult, TimeZone, Utc};

use log::debug;
use std::clone::Clone;
use std::convert::TryInto;

#[derive(Debug)]
pub struct ReportSummary {
    pub phases: Vec<PhaseStatistics>,
    pub average_duration: f32,
    pub pull_count: i32,
    pub total_time_spent_in_fights: f32,
}

impl std::fmt::Display for ReportSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f, "Total pulls: {0} with average duration {1:.1}s.\nA total of {2:.1}s was spent in battle with individual phase progress as follows: \n",
            self.pull_count,
            self.average_duration,
            self.total_time_spent_in_fights
        )?;
        for phase in &self.phases {
            write!(f, "{}\n", phase)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct PhaseStatistics {
    pub name: String,
    pub total_time_spent_secs: f32,
    pub clear_rate: f32,
    pub seen_rate: f32,
    pub seen_count: i32,
}

impl std::fmt::Display for PhaseStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let clear_rate_pct = self.clear_rate * 100.0;
        let seen_rate_pct = self.seen_rate * 100.0;
        if self.seen_count == 0 {
            write!(f, "**{0}**: This phase was never seen.", self.name)
        } else {
            write!(
                f,
                "**{0}**:\nA total of {1:.1}s was spent practicing this phase, with a clear rate of {2:.1}%.\nThis phase was seen {3} times ({4:.1}% of pulls)",
                self.name, self.total_time_spent_secs, clear_rate_pct, self.seen_count, seen_rate_pct
        )
        }
    }
}

pub fn summarise_report(fight_stats: Vec<FightStatistics>) -> ReportSummary {
    if fight_stats.len() == 0 {
        return ReportSummary {
            phases: Vec::new(),
            average_duration: 0.0,
            pull_count: 0,
            total_time_spent_in_fights: 0.0,
        };
    };
    let definitions: &Vec<PhaseDefinitionsPhase> = fight_stats[0].definitions;
    let mut phases = Vec::new();
    for phase in definitions {
        let mut time = 0.0;
        let mut seen_count: i32 = 0;
        let mut cleared_count: i32 = 0;
        for fight in &fight_stats {
            if let Some(idx) = fight
                .prog
                .iter()
                .position(|ph| ph.phase_name == phase.phase_name)
            {
                let phase_prog_data = &fight.prog[idx];
                time += phase_prog_data.phase_duration_secs;
                seen_count += 1;
                cleared_count += if phase_prog_data.phase_cleared == ClearedStatus::Clear {
                    1
                } else {
                    0
                };
            }
        }

        phases.push(PhaseStatistics {
            name: phase.phase_name.to_string(),
            total_time_spent_secs: time,
            clear_rate: cleared_count as f32 / seen_count as f32,
            seen_rate: seen_count as f32 / fight_stats.len() as f32,
            seen_count: seen_count,
        })
    }
    let mut total_time: f32 = 0.0;
    for fight in &fight_stats {
        total_time += fight.duration;
    }

    return ReportSummary {
        phases: phases,
        average_duration: total_time / fight_stats.len() as f32,
        pull_count: (fight_stats.len() as i32),
        total_time_spent_in_fights: total_time,
    };
}

pub fn get_report_stats(raw_data: ReportAnalysis) -> Vec<FightStatistics> {
    let mut res: Vec<FightStatistics> = Vec::new();
    let report_start_millis = raw_data.report_start;
    for fight in raw_data.fights {
        let stats: FightStatistics = get_pull_stats(fight, report_start_millis);
        res.push(stats);
    }
    return res;
}

pub fn get_pull_stats(raw_data: FightAnalysis, report_start_millis: u64) -> FightStatistics {
    let fight_start_time: Option<DateTime<Utc>> = (raw_data.start_time + report_start_millis)
        .try_into()
        .map_or(None, |millis: i64| match Utc.timestamp_millis_opt(millis) {
            LocalResult::Single(dt) => Some(dt),
            LocalResult::None => None,
            LocalResult::Ambiguous(_, _) => None,
        });
    let fight_end_time: Option<DateTime<Utc>> = (raw_data.end_time + report_start_millis)
        .try_into()
        .map_or(None, |millis: i64| match Utc.timestamp_millis_opt(millis) {
            LocalResult::Single(dt) => Some(dt),
            LocalResult::None => None,
            LocalResult::Ambiguous(_, _) => None,
        });
    let duration_millis: u64 = raw_data.end_time - raw_data.start_time;

    let mut phase_iter = raw_data.phases.iter().peekable();
    let mut phases_prog: Vec<PhaseProgress> = Vec::new();
    while let Some(phase) = phase_iter.next() {
        let name = phase.phase_name.clone();
        let duration_millis: u64;
        let cleared: ClearedStatus;
        //Not the last phase seen
        if let Some(next_phase) = phase_iter.peek() {
            duration_millis = phase.phase_end.unwrap_or(next_phase.phase_start) - phase.phase_start;
            cleared = ClearedStatus::Clear;
        } else {
            //Last phase but we have a well-defined end time
            if let Some(end_time) = phase.phase_end {
                duration_millis = end_time - phase.phase_start;
                cleared = ClearedStatus::Clear;
            //No defined end events, so just guess based on fflogs end time
            } else {
                duration_millis = raw_data.end_time - phase.phase_start;
                let definitions = raw_data.definitions;
                let total_fight_phases = definitions.len();
                cleared = definitions
                    .iter()
                    .position(|def| def.phase_name == phase.phase_name)
                    .map_or_else(
                        || ClearedStatus::Unknown,
                        |idx| {
                            if (total_fight_phases - 1) > idx {
                                ClearedStatus::Wiped
                            } else {
                                ClearedStatus::Unknown
                            }
                        },
                    );
            }
        }
        let res = PhaseProgress {
            phase_name: name,
            phase_duration_secs: duration_millis as f32 / 1000.0,
            phase_cleared: cleared,
        };
        phases_prog.push(res);
    }

    FightStatistics {
        fight_name: raw_data.fight_name.clone(),
        fight_start: fight_start_time,
        fight_end: fight_end_time,
        duration: duration_millis as f32 / 1000.0,
        prog: phases_prog,
        definitions: raw_data.definitions,
    }
}

pub struct LogAnalysisClient {
    fflogs_api_client: FFLogsApiClient,
    phase_definitions: PhaseDefinitionsCollection,
}

impl LogAnalysisClient {
    pub fn new(
        api_key: &str,
        definitions_dir: &str,
    ) -> Result<LogAnalysisClient, DefinitionsLoadError> {
        let api = new_fflogs_api_client(api_key);
        let definitions = load_definitions_files(definitions_dir)?;
        let res = LogAnalysisClient {
            fflogs_api_client: api,
            phase_definitions: definitions,
        };
        return Ok(res);
    }
}

#[derive(Debug)]
pub struct FightStatistics<'a> {
    fight_name: String,
    fight_start: Option<DateTime<Utc>>,
    fight_end: Option<DateTime<Utc>>,
    duration: f32,
    prog: Vec<PhaseProgress>,
    definitions: &'a Vec<PhaseDefinitionsPhase>,
}

#[derive(Debug)]
pub struct PhaseProgress {
    phase_name: String,
    phase_duration_secs: f32,
    phase_cleared: ClearedStatus,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClearedStatus {
    Clear,
    Wiped,
    Unknown,
}

pub fn convert_report_code(code_or_url: String) -> Result<String, AnalysisError> {
    lazy_static! {
        static ref URL_RE: Regex =
            Regex::new(r"(?:https?://)?(?:www.)?fflogs\.com/reports/(?P<code>[a-zA-Z0-9]{16})")
                .unwrap();
        static ref CODE_RE: Regex = Regex::new(r"(?P<code>[a-zA-Z0-9]{16})").unwrap();
    }
    URL_RE
        .captures(&code_or_url)
        .or(CODE_RE.captures(&code_or_url))
        .and_then(|caps| caps.name("code"))
        .map(|code| code.as_str().to_string())
        .ok_or(AnalysisError::InvalidReportCodeOrUrl)
}

pub async fn analyse_fights_by_name<'a>(
    report_code: String,
    name: String,
    analysis_client: &'a LogAnalysisClient,
) -> Result<ReportAnalysis<'a>, AnalysisError> {
    analyse_fights_from_report(
        report_code,
        |f| f.name == Some(name.clone()),
        analysis_client,
    )
    .await
}

pub async fn analyse_fights_from_report<'a, P>(
    report_code: String,
    pred: P,
    analysis_client: &'a LogAnalysisClient,
) -> Result<ReportAnalysis<'a>, AnalysisError>
where
    P: Fn(&Fight) -> bool,
{
    let client = &analysis_client.fflogs_api_client;
    let definitions = &analysis_client.phase_definitions;
    let mut fights: Vec<FightAnalysis> = Vec::new();
    let report_fights: ReportFightsList = request_fights(&report_code, true, &client)
        .await
        .map_err(|e| AnalysisError::ApiError(e))?;
    let matching_fights: Vec<&Fight> = report_fights.fights.iter().filter(|&f| pred(f)).collect();
    for fight in matching_fights.iter() {
        let fight_name = fight
            .name
            .as_ref()
            .cloned()
            .ok_or(AnalysisError::UnlabeledFight)?;
        let phase_definitions = definitions
            .get(&fight_name)
            .ok_or(AnalysisError::UnknownFightError(fight_name.clone()))?;
        let metadata = FightData {
            name: fight_name,
            report_code: report_code.clone(),
            start_time: fight.start_time,
            end_time: fight.end_time,
        };
        let fight_analysis: FightAnalysis = analyse_fight(
            fight.start_time,
            fight.end_time,
            phase_definitions,
            &client,
            &metadata,
        )
        .await?;
        fights.push(fight_analysis);
    }
    let res = ReportAnalysis {
        report_code: report_code,
        report_start: report_fights
            .start
            .ok_or(AnalysisError::UnspecifiedFightTime)?,
        report_end: report_fights
            .end
            .ok_or(AnalysisError::UnspecifiedFightTime)?,
        fights: fights,
    };
    return Ok(res);
}

#[derive(Debug)]
pub enum AnalysisError {
    ApiError(ApiError),
    UnknownFightError(String),
    InvalidEventMatchError,
    UnspecifiedFightTime,
    UnlabeledFight,
    NoMatchingFights,
    InvalidReportCodeOrUrl,
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            AnalysisError::ApiError(e) => format!(
                "Something went wrong when communicating with the FFLogs API: {:?}",
                e
            ),
            AnalysisError::UnknownFightError(f) => {
                format!("Phase definitions do not yet exist for {}.", f)
            }
            AnalysisError::InvalidEventMatchError => {
                "FFLogs API returned an unknown event type.".to_string()
            }
            AnalysisError::UnspecifiedFightTime => {
                "FFLogs API did not specify fight timings.".to_string()
            }
            AnalysisError::UnlabeledFight => "FFLogs API did not specify fight name.".to_string(),
            AnalysisError::NoMatchingFights => {
                "That report did not contain any fights matching the requested fight.".to_string()
            }
            AnalysisError::InvalidReportCodeOrUrl => {
                "That wasn't a valid report code or FFLogs url.".to_string()
            }
        };
        write!(f, "{}", msg)
    }
}

async fn analyse_fight<'a>(
    start_time: u64,
    end_time: u64,
    definitions: &'a Vec<PhaseDefinitionsPhase>,
    client: &FFLogsApiClient,
    metadata: &FightData,
) -> Result<FightAnalysis<'a>, AnalysisError> {
    let mut analysed_phases: Vec<RawPhaseData> = Vec::new();
    let mut latest_time_processed = start_time;

    for phase_definition in definitions.iter() {
        let phase_analysis: Option<RawPhaseData> = analyse_phase(
            latest_time_processed,
            end_time,
            phase_definition,
            client,
            metadata,
        )
        .await?;
        if let Some(phase) = phase_analysis {
            latest_time_processed = phase.phase_end.unwrap_or(phase.phase_start);
            analysed_phases.push(phase);
        } else {
            break;
        }
    }
    let mut phases_iter = analysed_phases.iter_mut().peekable();
    loop {
        let cur_phase: &mut RawPhaseData;
        let cur_phase_maybe = phases_iter.next();
        let next_phase: &&mut RawPhaseData;
        let next_phase_maybe = phases_iter.peek();
        match next_phase_maybe {
            Some(next) => next_phase = next,
            None => break,
        };
        match cur_phase_maybe {
            Some(cur) => cur_phase = cur,
            None => break,
        };
        if cur_phase.phase_end_event.is_none() {
            cur_phase.phase_end_event = Some(next_phase.phase_start_event.clone());
            cur_phase.phase_end = Some(next_phase.phase_start);
        }
    }
    let res = FightAnalysis {
        fight_name: metadata.name.clone(),
        report_code: metadata.report_code.clone(),
        definitions: definitions,
        start_time: metadata.start_time,
        end_time: metadata.end_time,
        phases: analysed_phases,
    };
    return Ok(res);
}

async fn analyse_phase(
    start_time: u64,
    end_time: u64,
    definition: &PhaseDefinitionsPhase,
    client: &FFLogsApiClient,
    metadata: &FightData,
) -> Result<Option<RawPhaseData>, AnalysisError> {
    let report_code = metadata.report_code.clone();
    debug!(
        "Now analysing phase {} for a fight in report {}.",
        &definition.phase_name, report_code
    );
    let mut res: RawPhaseData = Default::default();
    res.phase_name = definition.phase_name.clone();
    if let Some(marker) = &definition.start_marker {
        let matching_event: Option<ReportEvent> = marker
            .get_matching_event(report_code.clone(), start_time, end_time, client.clone())
            .await
            .map_err(|e| AnalysisError::ApiError(e))?;
        match matching_event {
            None => return Ok(None),
            Some(ev) => res.phase_start_event = ev,
        };
        res.phase_start = res
            .phase_start_event
            .get_timestamp()
            .ok_or(AnalysisError::InvalidEventMatchError)?;
    };

    if let Some(marker) = &definition.end_marker {
        let matching_event: Option<ReportEvent> = marker
            .get_matching_event(report_code.clone(), res.phase_start, end_time, client)
            .await
            .map_err(|e| AnalysisError::ApiError(e))?;
        res.phase_end_event = matching_event;
        res.phase_end = res
            .phase_end_event
            .as_ref()
            .and_then(|ev| ev.get_timestamp());
    };

    return Ok(Some(res));
}

struct FightData {
    name: String,
    report_code: String,
    start_time: u64,
    end_time: u64,
}

#[derive(Debug)]
pub struct ReportAnalysis<'a> {
    report_code: String,
    report_start: u64,
    report_end: u64,
    fights: Vec<FightAnalysis<'a>>,
}

#[derive(Debug)]
pub struct FightAnalysis<'a> {
    fight_name: String,
    report_code: String,
    definitions: &'a Vec<PhaseDefinitionsPhase>,
    start_time: u64,
    end_time: u64,
    phases: Vec<RawPhaseData>,
}

#[derive(Default, Debug)]
pub struct RawPhaseData {
    phase_name: String,
    phase_start: u64,
    phase_start_event: ReportEvent,
    phase_end: Option<u64>,
    phase_end_event: Option<ReportEvent>,
}
