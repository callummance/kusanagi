//! API calls and types which allow you to fetch a list of events that occurred during
//! an FFLogs report
use crate::fflogs_api::api::{fflogs_request, ApiError, FFLogsApiClient};
use crate::fflogs_api::types::{Ability, Resources, Source, Target};

use futures::future::BoxFuture;
use futures::stream::Stream;
use futures::task::{Context, Poll};
use http::uri::Uri;
use std::pin::*;

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use log::{debug, info, warn};
use std::fmt;

// //////////////////////////////// //
// ////// Request Structs  //////// //
// //////////////////////////////// //

pub async fn request_all_events(
    view: EventsView,
    report_code: &str,
    filters: EventFilters,
    client: &FFLogsApiClient,
) -> Result<Vec<ReportEvent>, ApiError> {
    let mut events_list: Vec<ReportEvent> = Vec::new();
    let mut updated_filters = filters.clone();
    loop {
        let mut evs_page =
            request_events(view, report_code, updated_filters.clone(), client).await?;
        events_list.append(&mut evs_page.events);
        match evs_page.next_page_timestamp {
            None => break,
            Some(nt) => {
                if nt < updated_filters.end {
                    updated_filters.start = nt;
                } else {
                    break;
                }
            }
        }
    }
    return Ok(events_list);
}

pub fn get_event_iterator<'a>(
    view: EventsView,
    report_code: &'a str,
    filters: EventFilters,
    client: &'a FFLogsApiClient,
) -> EventsStream<'a> {
    let init_events_list = ReportEventsList {
        events: Vec::new(),
        next_page_timestamp: Some(filters.start),
    };
    let res = EventsStream {
        view: view.clone(),
        report_code: report_code,
        filters: filters.clone(),
        client: client,
        events: init_events_list,
        current_event_position: 0,
        next_page: None,
    };
    return res;
}

pub struct EventsStream<'a> {
    view: EventsView,
    report_code: &'a str,
    filters: EventFilters,
    client: &'a FFLogsApiClient,
    events: ReportEventsList,
    current_event_position: usize,
    next_page: Option<BoxFuture<'a, Result<ReportEventsList, ApiError>>>,
}

impl<'a> Stream for EventsStream<'a> {
    type Item = Result<ReportEvent, ApiError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<ReportEvent, ApiError>>> {
        let remaining_events = self.events.events.len() - (self.current_event_position);
        //If there are no locally stored events left
        if remaining_events <= 0 {
            match self.events.next_page_timestamp {
                //If we have no locally stored events left and also no way to fetch more
                None => return Poll::Ready(None),
                //If we have run out of locally stored events but still have filters ready to get the next page
                Some(new_start) => {
                    if new_start >= self.filters.end {
                        return Poll::Ready(None);
                    } else {
                        self.filters.start = new_start;
                    }
                    let next_page_fut = match self.next_page.as_mut() {
                        None => {
                            let fut = request_events(
                                self.view,
                                self.report_code,
                                self.filters.clone(),
                                self.client,
                            );
                            let res = Box::pin(fut);
                            self.next_page = Some(res);
                            self.next_page.as_mut().unwrap()
                        }
                        Some(fut) => fut,
                    };
                    let next_page_completion = next_page_fut.as_mut().poll(cx);
                    match next_page_completion {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                        Poll::Ready(Ok(mut next_page)) => {
                            self.next_page = None;
                            if next_page.events.len() < 1 {
                                return Poll::Ready(None);
                            }
                            self.events.events.append(&mut next_page.events);
                            self.events.next_page_timestamp = next_page.next_page_timestamp;
                            let res = self.events.events[self.current_event_position].clone();
                            self.current_event_position += 1;
                            return Poll::Ready(Some(Ok(res)));
                        }
                    }
                }
            }
        //If we have more locally stored events, return them
        } else {
            let res = self.events.events[self.current_event_position].clone();
            self.current_event_position += 1;
            return Poll::Ready(Some(Ok(res)));
        }
    }
}

pub async fn request_events(
    view: EventsView,
    report_code: &str,
    filters: EventFilters,
    client: &FFLogsApiClient,
) -> Result<ReportEventsList, ApiError> {
    info!(
        "Making API request to request.events endpoint on report code {}.",
        report_code
    );
    let url = construct_url(&view, report_code, filters, client.api_key())?;
    let target_url = url.to_string();
    let resp: String = client.run_request(url).await?;
    let res: ReportEventsList = serde_json::from_str(&resp).map_err(|err| {
        warn!("Failed to decode response due to error {:?}.", err);
        debug!("Response contents: {:?}.", resp);
        return ApiError::ResponseFormatError(err);
    })?;
    if res
        .events
        .iter()
        .any(|e| e == &ReportEvent::UnparseableEvent)
    {
        warn!(
            "Unknown event type recieved when parsing API call to {}",
            target_url
        );
    }
    return Ok(res);
}

/// Attempts to construct the URL from which a request can be made to the report
/// events endpoint.
pub fn construct_url(
    view: &EventsView,
    report_code: &str,
    filters: EventFilters,
    api_key: &str,
) -> Result<Uri, ApiError> {
    let path = construct_path(&view, report_code);
    let query = QueryParams {
        filters: filters,
        api_key: api_key.to_owned(),
    };
    return fflogs_request(&path, query);
}

fn construct_path(view: &EventsView, report_code: &str) -> String {
    return format!("/v1/report/events/{}/{}", view, report_code);
}

/// Enum representing the different views that can be requested from the API.
/// This affects the types of event which will be returned.
#[derive(Copy, Clone)]
pub enum EventsView {
    Summary,
    DamageDone,
    DamageTaken,
    Healing,
    Casts,
    Summons,
    Buffs,
    Debuffs,
    Deaths,
    Threat,
    Resources,
    Interrupts,
    Dispels,
}

impl fmt::Display for EventsView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            EventsView::Summary => write!(f, "summary"),
            EventsView::DamageDone => write!(f, "damage-done"),
            EventsView::DamageTaken => write!(f, "damage-taken"),
            EventsView::Healing => write!(f, "healing"),
            EventsView::Casts => write!(f, "casts"),
            EventsView::Summons => write!(f, "summons"),
            EventsView::Buffs => write!(f, "buffs"),
            EventsView::Debuffs => write!(f, "debuffs"),
            EventsView::Deaths => write!(f, "deaths"),
            EventsView::Threat => write!(f, "threat"),
            EventsView::Resources => write!(f, "resources"),
            EventsView::Interrupts => write!(f, "interrupts"),
            EventsView::Dispels => write!(f, "dispeals"),
        }
    }
}

#[derive(Serialize)]
struct QueryParams {
    #[serde(flatten)]
    filters: EventFilters,
    api_key: String,
}

/// Filters which may be applied to the list of events during the API request
#[derive(Serialize, Default, Debug, Clone)]
pub struct EventFilters {
    pub start: u64,
    pub end: u64,
    pub hostility: Option<Hostility>,
    pub source_id: Option<i64>,
    pub source_instance: Option<i64>,
    pub source_class: Option<String>,
    pub target_id: Option<i64>,
    pub target_instance: Option<i64>,
    pub target_class: Option<String>,
    pub ability_id: Option<i64>,
    pub death: Option<i64>,
    pub options: Option<i64>,
    pub cutoff: Option<i64>,
    pub encounter: Option<i64>,
    pub wipes: Option<i64>,
    pub difficulty: Option<i64>,
    pub filter: Option<String>,
    pub translate: Option<bool>,
}

/// Whether an actor is hostile or friendly to the player character
#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, PartialEq, Copy)]
#[repr(u8)]
pub enum Hostility {
    Friendly = 0,
    Hostile = 1,
}

impl std::default::Default for Hostility {
    fn default() -> Self {
        return Hostility::Friendly;
    }
}

// //////////////////////////////// //
// ////// Response Structs //////// //
// //////////////////////////////// //
//TODO: Fix deserializing into default variant when fixed
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ReportEventsList {
    #[serde(rename = "events")] //, deserialize_with = "events_list_deserialize")]
    pub events: Vec<ReportEvent>,
    #[serde(rename = "nextPageTimestamp")]
    pub next_page_timestamp: Option<u64>,
}

fn _events_list_deserialize<'de, D>(deserializer: D) -> Result<Vec<ReportEvent>, D::Error>
where
    D: Deserializer<'de>,
{
    struct EventsVisitor {}

    impl<'de> Visitor<'de> for EventsVisitor {
        type Value = Vec<ReportEvent>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            return formatter.write_str("an array of report events");
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: SeqAccess<'de>,
        {
            let mut res = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            loop {
                match seq.next_element() {
                    Ok(Some(ev)) => res.push(ev),
                    Ok(None) => break,
                    Err(err) => {
                        //res.push(ReportEvent::UnparseableEvent(err.to_string()));
                        warn!(
                            "Found unknown event type whilst parsing events list: {}",
                            err
                        );
                    }
                };
            }
            return Ok(res);
        }
    }
    let visitor = EventsVisitor {};
    return deserializer.deserialize_any(visitor);
}

///A single event fired during an FFLogs report
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "type")]
pub enum ReportEvent {
    #[serde(rename = "calculateddamage")]
    CalculatedDamage(CalculatedDamage),
    #[serde(rename = "damage")]
    Damage(Damage),
    #[serde(rename = "calculatedheal")]
    CalculatedHeal(CalculatedHeal),
    #[serde(rename = "heal")]
    Heal(Heal),
    #[serde(rename = "begincast")]
    BeginCast(BeginCast),
    #[serde(rename = "cast")]
    Cast(Cast),
    #[serde(rename = "applybuff")]
    ApplyBuff(ApplyBuff),
    #[serde(rename = "refreshbuff")]
    RefreshBuff(RefreshBuff),
    #[serde(rename = "applybuffstack")]
    ApplyBuffStack(ApplyBuffStack),
    #[serde(rename = "removebuff")]
    RemoveBuff(RemoveBuff),
    #[serde(rename = "removebuffstack")]
    RemoveBuffStack(RemoveBuffStack),
    #[serde(rename = "applydebuff")]
    ApplyDebuff(ApplyDebuff),
    #[serde(rename = "refreshdebuff")]
    RefreshDebuff(RefreshDebuff),
    #[serde(rename = "applydebuffstack")]
    ApplyDebuffStack(ApplyDebuffStack),
    #[serde(rename = "removedebuff")]
    RemoveDebuff(RemoveDebuff),
    #[serde(rename = "removedebuffstack")]
    RemoveDebuffStack(RemoveDebuffStack),
    #[serde(rename = "death")]
    Death(Death),
    #[serde(rename = "limitbreakupdate")]
    LimitBreakUpdate(LimitBreakUpdate),
    #[serde(other)]
    UnparseableEvent,
}

impl Default for ReportEvent {
    fn default() -> ReportEvent {
        return ReportEvent::UnparseableEvent;
    }
}

impl ReportEvent {
    pub fn get_timestamp(&self) -> Option<u64> {
        match self {
            ReportEvent::CalculatedDamage(ev) => Some(ev.timestamp),
            ReportEvent::Damage(ev) => Some(ev.timestamp),
            ReportEvent::CalculatedHeal(ev) => Some(ev.timestamp),
            ReportEvent::Heal(ev) => Some(ev.timestamp),
            ReportEvent::BeginCast(ev) => Some(ev.timestamp),
            ReportEvent::Cast(ev) => Some(ev.timestamp),
            ReportEvent::ApplyBuff(ev) => Some(ev.timestamp),
            ReportEvent::RefreshBuff(ev) => Some(ev.timestamp),
            ReportEvent::ApplyBuffStack(ev) => Some(ev.timestamp),
            ReportEvent::RemoveBuff(ev) => Some(ev.timestamp),
            ReportEvent::RemoveBuffStack(ev) => Some(ev.timestamp),
            ReportEvent::ApplyDebuff(ev) => Some(ev.timestamp),
            ReportEvent::RefreshDebuff(ev) => Some(ev.timestamp),
            ReportEvent::ApplyDebuffStack(ev) => Some(ev.timestamp),
            ReportEvent::RemoveDebuff(ev) => Some(ev.timestamp),
            ReportEvent::RemoveDebuffStack(ev) => Some(ev.timestamp),
            ReportEvent::Death(ev) => Some(ev.timestamp),
            ReportEvent::LimitBreakUpdate(ev) => Some(ev.timestamp),
            ReportEvent::UnparseableEvent => None,
        }
    }
}

///Event fired when damage has snapshot and been calculated but has not yet been applied
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CalculatedDamage {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "hitType")]
    pub hit_type: Option<i64>,
    #[serde(rename = "amount")]
    pub amount: Option<i64>,
    #[serde(rename = "absorbed")]
    pub absorbed_amount: Option<i64>,
    #[serde(rename = "multistrike")]
    pub multistrike: Option<bool>,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when damage is applied
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Damage {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "hitType")]
    pub hit_type: Option<i64>,
    #[serde(rename = "amount")]
    pub amount: Option<i64>,
    #[serde(rename = "absorbed")]
    pub absorbed_amount: Option<i64>,
    #[serde(rename = "multistrike")]
    pub multistrike: Option<bool>,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a heal amount is calculated
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CalculatedHeal {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "hitType")]
    pub hit_type: Option<i64>,
    #[serde(rename = "amount")]
    pub amount: Option<i64>,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Heal {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "hitType")]
    pub hit_type: Option<i64>,
    #[serde(rename = "amount")]
    pub amount: Option<i64>,
    #[serde(rename = "overheal")]
    pub overheal: Option<i64>,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
    #[serde(rename = "targetResources")]
    pub target_resources: Option<Resources>,
}

///Event fired when an actor begins a cast
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct BeginCast {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a cast completes
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Cast {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a buff is applied
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ApplyBuff {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when the duration on a buff is refreshed
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RefreshBuff {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a stacking buff is applied or the number of stacks in increased
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ApplyBuffStack {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "stack")]
    pub stack_count: i64,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a buff falls off or is removed
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RemoveBuff {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a stacking buff falls off or is removed
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RemoveBuffStack {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "stack")]
    pub stack_count: i64,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a debuff is applied
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ApplyDebuff {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when the duration on a debuff is refreshed
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RefreshDebuff {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a stacking debuff is applied or the number of stacks in increased
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ApplyDebuffStack {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "stack")]
    pub stack_count: i64,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a debuff falls off or is removed
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RemoveDebuff {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

///Event fired when a stacking debuff falls off or is removed
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RemoveDebuffStack {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Ability,
    #[serde(rename = "stack")]
    pub stack_count: i64,
    #[serde(rename = "debugMultiplier")]
    pub debug_multiplier: Option<f64>,
    #[serde(rename = "packetID")]
    pub packet_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Death {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub target: Option<Target>,
    #[serde(rename = "ability")]
    pub ability: Option<Ability>,
    #[serde(rename = "killerID")]
    pub killer_id: Option<i64>,
    #[serde(rename = "killingAbility")]
    pub killing_ability: Option<Ability>,
}
///Event fired when the party's limit break charge updates
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct LimitBreakUpdate {
    #[serde(rename = "timestamp")]
    pub timestamp: u64,
    #[serde(rename = "value")]
    pub value: i32,
    #[serde(rename = "bars")]
    pub bars: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fflogs_api::types::{Ability, Resources, Source, Target};

    #[test]
    fn test_calcdamage_deserialization() {
        let msg = r#"{"timestamp":1335873,"type":"calculateddamage","sourceID":75,"sourceIsFriendly":true,"targetID":82,"targetIsFriendly":false,"ability":{"name":"Blizzard III","guid":154,"type":1024,"abilityIcon":"000000-000456.png"},"hitType":1,"amount":8417,"absorbed":0,"multistrike":true,"debugMultiplier":1.0,"packetID":41251,"sourceResources":{"hitPoints":48967,"maxHitPoints":48967,"mp":10000,"maxMP":10000,"tp":0,"maxTP":1000,"x":10115,"y":10984,"facing":-308,"absorb":0},"targetResources":{"hitPoints":5293335,"maxHitPoints":5293335,"mp":10000,"maxMP":34464,"tp":0,"maxTP":1000,"x":10000,"y":9000,"facing":0}}"#;
        let res = ReportEvent::CalculatedDamage(CalculatedDamage {
            timestamp: 1335873,
            source: Source {
                id: Some(75),
                source_data: None,
                is_friendly: true,
                resources: Some(Resources {
                    hp: Some(48967),
                    max_hp: Some(48967),
                    mp: Some(10000),
                    max_mp: Some(10000),
                    tp: Some(0),
                    max_tp: Some(1000),
                    x: Some(10115),
                    y: Some(10984),
                    facing: Some(-308),
                    absorb: Some(0),
                }),
            },
            target: Some(Target {
                id: Some(82),
                target_data: None,
                is_friendly: false,
                resources: Some(Resources {
                    hp: Some(5293335),
                    max_hp: Some(5293335),
                    mp: Some(10000),
                    max_mp: Some(34464),
                    tp: Some(0),
                    max_tp: Some(1000),
                    x: Some(10000),
                    y: Some(9000),
                    facing: Some(0),
                    absorb: None,
                }),
            }),
            ability: Ability {
                name: "Blizzard III".to_string(),
                guid: 154,
                ability_type: 1024,
                icon: Some("000000-000456.png".to_string()),
            },
            hit_type: Some(1),
            amount: Some(8417),
            absorbed_amount: Some(0),
            multistrike: Some(true),
            debug_multiplier: Some(1.0),
            packet_id: Some(41251),
        });
        let tgt: ReportEvent = serde_json::from_str(msg).unwrap();
        assert_eq!(tgt, res);
    }
}
