//! The `types` module contains types shared between multiple API data types.
use serde::{Deserialize, Serialize};

///The source for an event, found in a number of [`ReportEvent`] types
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Source {
    #[serde(rename = "sourceID")]
    pub id: Option<i64>,
    #[serde(rename = "source")]
    pub source_data: Option<ActorData>,
    #[serde(rename = "sourceIsFriendly")]
    pub is_friendly: bool,
    #[serde(rename = "sourceResources")]
    pub resources: Option<Resources>,
}

impl Source {
    pub fn get_id(&self) -> Option<i64> {
        return match self.id {
            Some(id) => Some(id),
            None => self.source_data.as_ref().map(|actor| actor.guid),
        };
    }
}

///An in-game character or NPC which can cause or be the target of an event
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ActorData {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "id")]
    pub id: i64,
    #[serde(rename = "guid")]
    pub guid: i64,
    #[serde(rename = "type")]
    pub actor_type: String,
    #[serde(rename = "icon")]
    pub icon: Option<String>,
}

///The target for an event, found in a number of [`ReportEvent`] types
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Target {
    #[serde(rename = "targetID")]
    pub id: Option<i64>,
    #[serde(rename = "target")]
    pub target_data: Option<ActorData>,
    #[serde(rename = "targetIsFriendly")]
    pub is_friendly: bool,
    #[serde(rename = "targetResources")]
    pub resources: Option<Resources>,
}

impl Target {
    pub fn get_id(&self) -> Option<i64> {
        return match self.id {
            Some(id) => Some(id),
            None => self.target_data.as_ref().map(|actor| actor.guid),
        };
    }
}

///Details on an ability used in a [`ReportEvent`], can also represent a buff
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Ability {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "guid")]
    pub guid: i64,
    #[serde(rename = "type")]
    pub ability_type: i64,
    #[serde(rename = "abilityIcon")]
    pub icon: Option<String>,
}

///Details on the resources of an actor at a given point in time
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Resources {
    #[serde(rename = "hitPoints")]
    pub hp: Option<i64>,
    #[serde(rename = "maxHitPoints")]
    pub max_hp: Option<i64>,
    #[serde(rename = "mp")]
    pub mp: Option<i64>,
    #[serde(rename = "maxMP")]
    pub max_mp: Option<i64>,
    #[serde(rename = "tp")]
    pub tp: Option<i64>,
    #[serde(rename = "maxTP")]
    pub max_tp: Option<i64>,
    #[serde(rename = "x")]
    pub x: Option<i64>,
    #[serde(rename = "y")]
    pub y: Option<i64>,
    #[serde(rename = "facing")]
    pub facing: Option<i64>,
    #[serde(rename = "absorb")]
    pub absorb: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Unit {
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "id")]
    id: Option<i64>,
    #[serde(rename = "guid")]
    guid: Option<i64>,
    #[serde(rename = "type")]
    unit_type: Option<String>,
    #[serde(rename = "server")]
    server: Option<String>,
    #[serde(rename = "icon")]
    icon: Option<String>,
    #[serde(rename = "petOwner")]
    pet_owner: Option<i64>,
    #[serde(rename = "fights")]
    fights: Vec<FightLink>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct FightLink {
    #[serde(rename = "id")]
    fight_id: i64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Instance {
    #[serde(rename = "boss")]
    boss: Option<i64>,
    #[serde(rename = "phases")]
    phases: Option<Vec<String>>,
}
