// Copyright (c) 2018 Ivo Wetzel

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


// Crates ---------------------------------------------------------------------
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate bincode;
extern crate rand;
#[macro_use]
extern crate log;


// STD Dependencies -----------------------------------------------------------
use std::hash::Hash;
use std::net::SocketAddr;
use std::fmt::{Display, Debug};


// External Dependencies ------------------------------------------------------
use serde::ser::Serialize;
use serde::de::DeserializeOwned;


// Modules --------------------------------------------------------------------
mod client;
mod server;


// Traits ---------------------------------------------------------------------
pub trait NetworkProperty: Hash + Eq + Serialize + DeserializeOwned + Display + Debug + Clone {}

impl NetworkProperty for String {}

impl NetworkProperty for u8 {}
impl NetworkProperty for u16 {}
impl NetworkProperty for u32 {}
impl NetworkProperty for u64 {}

impl NetworkProperty for i8 {}
impl NetworkProperty for i16 {}
impl NetworkProperty for i32 {}
impl NetworkProperty for i64 {}

pub trait NetworkConfig: Debug + Clone {
    type LobbyId: NetworkProperty;
    type LobbyPayload: NetworkProperty;
    type ConnectionIdentifier: NetworkProperty;
    type PreferenceKey: NetworkProperty;
    type PreferenceValue: NetworkProperty;
}


// Enums ----------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = ""))]
enum Message<C> where C: NetworkConfig {
    IdentifyAction(C::ConnectionIdentifier),
    ReadyAction,
    UdpAddressAction(SocketAddr),
    LobbyCreateAction(C::LobbyId),
    LobbyJoinAction(C::LobbyId, Option<C::LobbyPayload>),
    LobbyPreferenceAction(C::PreferenceKey, C::PreferenceValue, bool),
    LobbyLeaveAction,
    LobbyStartAction,
    LobbyJoinEvent {
        id: C::LobbyId,
        addr: SocketAddr,
        ident: C::ConnectionIdentifier,
        is_owner: bool
    },
    LobbyJoinResponseAction(SocketAddr, bool),
    IdentifyEvent(u32),
    UdpAddressEvent(SocketAddr),
    LobbyLeaveEvent(SocketAddr),
    LobbyStartEvent(C::LobbyId),
    LobbyJoinRequestEvent {
        id: C::LobbyId,
        ident: C::ConnectionIdentifier,
        addr: SocketAddr,
        payload: Option<C::LobbyPayload>
    },
    LobbyPreferenceEvent(C::LobbyId, C::PreferenceKey, C::PreferenceValue),
    LobbyPreferenceRequestEvent(C::LobbyId, C::ConnectionIdentifier, SocketAddr, C::PreferenceKey, C::PreferenceValue),
    LobbyCreateEvent(C::LobbyId),
    LobbyDestroyEvent(C::LobbyId),
    InvalidLobby(C::LobbyId),
    InvalidAddress,
    InvalidAction
}


// Re-Exports ------------------------------------------------------------------
pub use self::client::{Client, Event};
pub use self::server::Server;

