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


// STD Dependencies -----------------------------------------------------------
use std::hash::Hash;
use std::fmt::Display;
use std::net::SocketAddr;


// External Dependencies ------------------------------------------------------
use serde::ser::Serialize;
use serde::de::DeserializeOwned;


// Modules --------------------------------------------------------------------
mod client;
mod server;


// Traits ---------------------------------------------------------------------
pub trait NetworkProperty: Hash + Eq + Serialize + DeserializeOwned + Display + Clone {}
impl NetworkProperty for String {}

/*
pub struct Preference<K: NetworkProperty, V: NetworkProperty> {
    key: K,
    value: V,
    public: bool
}

impl<K: NetworkProperty, V: NetworkProperty> Preference<K, V> {

    pub fn public(key: K, value: V) -> Self {
        Self {
            key,
            value,
            public: true
        }
    }

    pub fn private(key: K, value: V) -> Self {
        Self {
            key,
            value,
            public: false
        }
    }

    fn key()

}*/


// Enums ----------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = ""))]
enum Message<T, S, I, K, V> where T: NetworkProperty,
                               S: NetworkProperty,
                               I: NetworkProperty,
                               K: NetworkProperty,
                               V: NetworkProperty
{
    IdentifyAction(I),
    ReadyAction,
    UdpAddressAction(SocketAddr),
    LobbyJoinAction {
        id: T,
        secret: Option<S>
    },
    LobbyPreferenceAction(K, V, bool),
    LobbyLeaveAction,
    LobbyStartAction,
    LobbyJoinEvent {
        id: T,
        addr: SocketAddr,
        ident: I,
        is_local: bool,
        is_owner: bool
    },
    IdentifyEvent(u32),
    UdpAddressEvent(SocketAddr),
    LobbyLeaveEvent(SocketAddr),
    LobbyStartEvent(T),
    LobbyPreferenceEvent(T, K, V),
    LobbyCreateEvent(T),
    LobbyDestroyEvent(T),
    InvalidLobby(T),
    InvalidLobbySecret(T),
    InvalidAddress,
    InvalidAction
}


// Re-Exports ------------------------------------------------------------------
pub use self::client::{Client, Event};
pub use self::server::Server;

