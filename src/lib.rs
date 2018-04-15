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
use std::fmt;
use std::hash::Hash;
use std::ops::Deref;
use std::collections::HashMap;
use std::fmt::{Display, Debug};
use std::net::{IpAddr, Ipv4Addr, UdpSocket, SocketAddr};


// External Dependencies ------------------------------------------------------
use rand::Rng;
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

pub trait NetworkConfig: Debug + Clone + Serialize + DeserializeOwned {
    type LobbyId: NetworkProperty;
    type LobbyPayload: NetworkProperty;
    type ConnectionIdentifier: NetworkProperty;
    type PreferenceKey: NetworkProperty;
    type PreferenceValue: NetworkProperty;
}


// Types ----------------------------------------------------------------------
#[derive(Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
struct UdpToken(u32);

impl UdpToken {
    fn new() -> Self {
        UdpToken(rand::thread_rng().next_u32())
    }

    fn into_buffer(self) -> [u8; 8] {
        [
            14, 71, 128, 5,
            (self.0 >> 24) as u8,
            (self.0 >> 16) as u8,
            (self.0 >> 8) as u8,
            (self.0) as u8
        ]
    }

    fn try_from_buffer(buffer: &[u8], len: usize) -> Option<Self> {
        if len == 8 && buffer[0] == 14 && buffer[1] == 71 && buffer[2] == 128 && buffer[3] == 5 {
            Some(
                UdpToken(
                    (u32::from(buffer[4]) << 24) |
                    (u32::from(buffer[5]) << 16) |
                    (u32::from(buffer[6]) << 8) |
                    u32::from(buffer[7])
                )
            )

        } else {
            None
        }

    }
}

impl Deref for UdpToken {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct UdpAddress(SocketAddr);

impl UdpAddress {

    fn new(addr: SocketAddr) -> Self {
        UdpAddress(addr)
    }

    fn empty() -> Self {
        UdpAddress(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0))
    }

    fn is_valid(&self) -> bool {
        self.0.port() != 0
    }

}

impl fmt::Display for UdpAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for UdpAddress {
    type Target = SocketAddr;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
struct TcpAddress(SocketAddr);

impl TcpAddress {
    fn new(addr: SocketAddr) -> Self {
        TcpAddress(addr)
    }
}

impl fmt::Display for TcpAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for TcpAddress {
    type Target = SocketAddr;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}


// Enums ----------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = ""))]
enum Message<C> where C: NetworkConfig {
    ClientAction(ClientAction<C>),
    ServerEvent(ServerEvent<C>)
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = ""))]
enum ClientAction<C> where C: NetworkConfig {
    Identify(C::ConnectionIdentifier),
    Ready,
    UdpAddress(UdpAddress),
    LobbyCreate(C::LobbyId),
    LobbyJoin(C::LobbyId, Option<C::LobbyPayload>),
    LobbyPreference {
        key: C::PreferenceKey,
        value: C::PreferenceValue,
        is_public: bool
    },
    LobbyLeave,
    LobbyStart,
    LobbyJoinResponse(UdpAddress, bool),
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = ""))]
enum ServerEvent<C> where C: NetworkConfig {
    Identify(UdpToken),
    UdpAddress(UdpAddress),
    LobbyJoin {
        id: C::LobbyId,
        addr: UdpAddress,
        ident: C::ConnectionIdentifier,
        is_owner: bool
    },
    LobbyLeave(UdpAddress),
    LobbyStart(C::LobbyId),
    LobbyJoinRequest {
        id: C::LobbyId,
        ident: C::ConnectionIdentifier,
        addr: UdpAddress,
        payload: Option<C::LobbyPayload>
    },
    LobbyPreference {
        id: C::LobbyId,
        key: C::PreferenceKey,
        value: C::PreferenceValue
    },
    LobbyPreferenceRequest {
        id: C::LobbyId,
        ident: C::ConnectionIdentifier,
        addr: UdpAddress,
        key: C::PreferenceKey,
        value: C::PreferenceValue
    },
    LobbyCreate(C::LobbyId),
    LobbyDestroy(C::LobbyId),
    Error(Error<C::LobbyId>)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(bound(deserialize = ""))]
pub enum Error<LobbyId: NetworkProperty> {
    AlreadyIdentified,
    AlreadyInLobby(LobbyId),
    NotInAnyLobby,
    LobbyJoinDenied(LobbyId),
    LobbyAlreadyExists(LobbyId),
    LobbyNotFound(LobbyId),
    NoUdpAddress,
    NoIdentification
}

pub enum Event<C: NetworkConfig> {
    Connected,
    Disconnected,
    Ready(C::ConnectionIdentifier, UdpAddress),
    LobbyJoined(C::LobbyId, Vec<Connection<C>>, HashMap<C::PreferenceKey, C::PreferenceValue>),
    LobbyJoinRequest(C::LobbyId, Connection<C>, Option<C::LobbyPayload>),
    LobbyPreferenceRequest(C::LobbyId, Connection<C>, C::PreferenceKey, C::PreferenceValue),
    LobbyUpdated(C::LobbyId, Vec<Connection<C>>, HashMap<C::PreferenceKey, C::PreferenceValue>),
    LobbyLeft(C::LobbyId),
    LobbyStarted(C::LobbyId, UdpSocket, Vec<Connection<C>>, HashMap<C::PreferenceKey, C::PreferenceValue>),
    Error(Error<C::LobbyId>)
}


// Re-Exports ------------------------------------------------------------------
pub use self::client::{Client, Connection};
pub use self::server::Server;

