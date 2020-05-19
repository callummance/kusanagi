use crate::fflogs_api::api::{ApiError, FFLogsApiClient};
use crate::fflogs_api::report::events::{
    get_event_iterator, EventFilters, EventsView, Hostility, ReportEvent,
};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use log::trace;

use futures::future;
use futures::stream::Stream;
use futures::TryStreamExt;
use std::fs::{read_dir, File};
use std::io::Read;
use toml;

use log::{error, info};

pub fn load_definitions_files(
    dir: &str,
) -> Result<PhaseDefinitionsCollection, DefinitionsLoadError> {
    let mut files = read_dir(dir).map_err(|e| DefinitionsLoadError::FileIOError(e))?;
    let mut res: HashMap<String, Vec<PhaseDefinitionsPhase>> = HashMap::new();
    let read_result: Result<(), DefinitionsLoadError> = files.try_for_each(|def_file| {
        let dir_entry = def_file.map_err(|e| DefinitionsLoadError::FileIOError(e))?;
        let f_type = dir_entry
            .file_type()
            .map_err(|e| DefinitionsLoadError::FileIOError(e))?;
        if f_type.is_dir() {
            return Ok(());
        };
        let f_name = dir_entry.file_name().to_string_lossy().to_string();
        if f_name.ends_with(".toml") {
            let path = dir_entry.path().to_string_lossy().to_string();
            let defs = load_definitions_file(&path)?;
            res.insert(defs.name, defs.phases);
            return Ok(());
        } else {
            return Ok(());
        };
    });
    match read_result {
        Err(e) => {
            error!(
                "An error occurred whilst decoding phase definitions: {:?}",
                e
            );
            return Err(e);
        }
        Ok(_) => {
            info!("Successfully loaded definitions files.");
            return Ok(PhaseDefinitionsCollection(res));
        }
    }
}

fn load_definitions_file(file_path: &str) -> Result<PhaseDefinitions, DefinitionsLoadError> {
    let mut file = File::open(file_path).map_err(|e| DefinitionsLoadError::FileIOError(e))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| DefinitionsLoadError::FileIOError(e))?;
    let decoded: PhaseDefinitions =
        toml::from_str(&contents).map_err(|e| DefinitionsLoadError::FileDecodeError(e))?;
    return Ok(decoded);
}

#[derive(Debug)]
pub enum DefinitionsLoadError {
    FileIOError(std::io::Error),
    FileDecodeError(toml::de::Error),
}

#[derive(Debug)]
pub struct PhaseDefinitionsCollection(HashMap<String, Vec<PhaseDefinitionsPhase>>);

impl PhaseDefinitionsCollection {
    pub fn get(&self, fight_name: &str) -> Option<&Vec<PhaseDefinitionsPhase>> {
        return self.0.get(fight_name);
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct PhaseDefinitions {
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "phase")]
    phases: Vec<PhaseDefinitionsPhase>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct PhaseDefinitionsPhase {
    #[serde(rename = "name")]
    pub phase_name: String,
    #[serde(rename = "startMarker")]
    pub start_marker: Option<PhaseMarker>,
    #[serde(rename = "endMarker")]
    pub end_marker: Option<PhaseMarker>,
}

impl PhaseMarker {
    pub fn check_event(&self, ev: &ReportEvent) -> bool {
        match self {
            PhaseMarker::FightStartMarker => true,
            PhaseMarker::EventMarker(marker) => marker.compare_to_event(ev),
        }
    }

    pub fn create_event_filters(&self) -> (EventsView, EventFilters) {
        match self {
            PhaseMarker::FightStartMarker => (EventsView::Summary, Default::default()),
            PhaseMarker::EventMarker(marker) => marker.create_event_filters(),
        }
    }

    pub async fn get_matching_event(
        &self,
        report_code: String,
        start_time: u64,
        end_time: u64,
        client: &FFLogsApiClient,
    ) -> Result<Option<ReportEvent>, ApiError> {
        match self {
            PhaseMarker::EventMarker(e) => {
                e.get_matching_event(report_code, start_time, end_time, client)
                    .await
            }
            PhaseMarker::FightStartMarker => {
                let (view, mut filters) = self.create_event_filters();
                filters.start = start_time;
                filters.end = end_time;
                let mut events_stream = get_event_iterator(view, &report_code, filters, &client);
                events_stream.try_next().await
            }
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(tag = "type")]
pub enum PhaseMarker {
    #[serde(rename = "fightStart")]
    FightStartMarker,
    #[serde(rename = "event")]
    EventMarker(EventMarker),
}

impl EventMarker {
    pub fn compare_to_event(&self, ev: &ReportEvent) -> bool {
        match self {
            EventMarker::BeginCast(marker) => {
                if let ReportEvent::BeginCast(ref ev_data) = ev {
                    let res = marker.ability_id == ev_data.ability.guid;
                    if !res {
                        trace!(
                            "Discarded BeginCast event for marker {:?} due to non-matching data: {:?}",
                            self,
                            ev
                        );
                    } else {
                        trace!("Accepted event {:?} as match for marker {:?}", ev, self);
                    };
                    res
                } else {
                    trace!(
                        "Discarded event for marker {:?} due to non-matching event_type (Expected BeginCast): {:?}",
                        self,
                        ev
                    );
                    false
                }
            }
            EventMarker::EndCast(marker) => {
                if let ReportEvent::Cast(ref ev_data) = ev {
                    let res = marker.ability_id == ev_data.ability.guid;
                    if !res {
                        trace!(
                            "Discarded EndCast event for marker {:?} due to non-matching data: {:?}",
                            self,
                            ev
                        );
                    } else {
                        trace!("Accepted event {:?} as match for marker {:?}", ev, self);
                    };
                    res
                } else {
                    trace!(
                        "Discarded event for marker {:?} due to non-matching event_type (Expected Cast): {:?}",
                        self,
                        ev
                    );
                    false
                }
            }
            EventMarker::Death(marker) => {
                if let ReportEvent::Death(ref ev_data) = ev {
                    let res = ev_data
                        .target
                        .as_ref()
                        .and_then(|t| t.get_id())
                        .map_or(false, |id| id == marker.target_id);
                    if !res {
                        trace!(
                            "Discarded Death event for marker {:?} due to non-matching data: {:?}",
                            self,
                            ev
                        );
                    } else {
                        trace!("Accepted event {:?} as match for marker {:?}", ev, self);
                    };
                    res
                } else {
                    trace!(
                        "Discarded event for marker {:?} due to non-matching event_type (Expected Death): {:?}",
                        self,
                        ev
                    );
                    false
                }
            }
        }
    }

    pub fn create_event_filters(&self) -> (EventsView, EventFilters) {
        let mut res: EventFilters = Default::default();
        let view: EventsView;
        match self {
            EventMarker::BeginCast(marker) => {
                res.ability_id = Some(marker.ability_id);
                res.hostility = Some(marker.hostility.unwrap_or(Hostility::Hostile));
                view = EventsView::Casts;
            }
            EventMarker::EndCast(marker) => {
                res.ability_id = Some(marker.ability_id);
                res.hostility = Some(marker.hostility.unwrap_or(Hostility::Hostile));
                view = EventsView::Casts;
            }
            EventMarker::Death(marker) => {
                res.target_id = Some(marker.target_id);
                res.hostility = Some(marker.hostility.unwrap_or(Hostility::Hostile));
                view = EventsView::Deaths;
            }
        }
        return (view, res);
    }
    async fn choose_from_matching(
        &self,
        mut events: impl Stream<Item = Result<ReportEvent, ApiError>> + Unpin,
    ) -> Result<Option<ReportEvent>, ApiError> {
        let mut skip_count = match self {
            EventMarker::BeginCast(m) => m.instance_no.unwrap_or(0),
            EventMarker::EndCast(m) => m.instance_no.unwrap_or(0),
            EventMarker::Death(m) => m.instance_no.unwrap_or(0),
        };
        while skip_count > 0 {
            let _ = events.try_next().await?;
            skip_count -= 1;
        }
        return events.try_next().await;
    }

    pub async fn get_matching_event(
        &self,
        report_code: String,
        start_time: u64,
        end_time: u64,
        client: &FFLogsApiClient,
    ) -> Result<Option<ReportEvent>, ApiError> {
        let (view, mut filters) = self.create_event_filters();
        filters.start = start_time;
        filters.end = end_time;
        let events_stream = get_event_iterator(view, &report_code, filters, &client);
        let matches = events_stream.try_filter(|ev| future::ready(self.compare_to_event(ev)));
        let chosen = self.choose_from_matching(matches).await;
        return chosen;
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(tag = "evType")]
pub enum EventMarker {
    #[serde(rename = "BeginCast")]
    BeginCast(BeginCastMarker),
    #[serde(rename = "Cast")]
    EndCast(EndCastMarker),
    #[serde(rename = "Death")]
    Death(DeathMarker),
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct BeginCastMarker {
    #[serde(rename = "abilityId")]
    ability_id: i64,
    #[serde(rename = "instanceNo")]
    instance_no: Option<i32>,
    #[serde(rename = "eventHostility")]
    hostility: Option<Hostility>,
}
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct EndCastMarker {
    #[serde(rename = "abilityId")]
    ability_id: i64,
    #[serde(rename = "instanceNo")]
    instance_no: Option<i32>,
    #[serde(rename = "eventHostility")]
    hostility: Option<Hostility>,
}
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct DeathMarker {
    #[serde(rename = "targetId")]
    target_id: i64,
    #[serde(rename = "instanceNo")]
    instance_no: Option<i32>,
    #[serde(rename = "eventHostility")]
    hostility: Option<Hostility>,
}
