// Copyright (c) 2018 Ivo Wetzel

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


// STD Dependencies -----------------------------------------------------------
use std::marker::PhantomData;
use std::collections::HashMap;
use std::io::{Error, ErrorKind, Read, Write};
use std::net::{UdpSocket, TcpStream, ToSocketAddrs, SocketAddr};


// External Dependencies ------------------------------------------------------
use bincode;


// Internal Dependencies ------------------------------------------------------
use ::{NetworkProperty, Message};


pub enum Event<
    LobbyId: NetworkProperty,
    Identifier: NetworkProperty
> {
    Connected,
    Ready(Identifier, SocketAddr),
    LobbyJoined(LobbyId, Vec<(Identifier, SocketAddr)>),
    LobbyUpdated(LobbyId, Vec<(Identifier, SocketAddr)>),
    LobbyLeft(LobbyId),
    LobbyStarted(LobbyId, UdpSocket, Vec<(Identifier, SocketAddr)>)
}

// Client Implementation ------------------------------------------------------
pub struct Client<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty
>{
    stream: TcpStream,
    connected: bool,
    lobby: Option<Lobby<LobbyId, Identifier, Key, Value>>,
    // lobbies: HashMap<LobbyId, Lobby<LobbyId, Key, Value>>,
    udp_socket: UdpSocket,
    udp_token: Option<u32>,
    udp_address: Option<SocketAddr>,
    ident: Identifier,
    _secret: PhantomData<LobbySecret>
}

impl<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty

> Client<LobbyId, LobbySecret, Identifier, Key, Value> {

    pub fn new<A: ToSocketAddrs>(address: A, ident: Identifier) -> Result<Self, Error> {
        let stream = TcpStream::connect(address)?;
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            lobby: None,
            connected: false,
            // lobbies: HashMap::new(),
            udp_socket: UdpSocket::bind("0.0.0.0:0")?,
            udp_token: None,
            udp_address: None,
            ident,
            _secret: PhantomData
        })
    }

    pub fn identify(&mut self, ident: Identifier) {
        self.send(Message::IdentifyAction(ident));
    }

    pub fn create_lobby(&mut self, id: LobbyId) -> bool {
        self.send(Message::LobbyJoinAction {
            id,
            secret: None
        });
        true
    }

    pub fn join_lobby(&mut self, id: LobbyId) -> bool {
        false
    }

    pub fn events(&mut self) -> impl Iterator<Item = Event<LobbyId, Identifier>> {

        let mut events = Vec::new();
        if !self.connected {
            events.push(Event::Connected);
            self.connected = true;
        }

        match self.receive() {
            Ok(messages) => for m in messages {
                match m {
                    Message::IdentifyEvent(token) => {
                        self.udp_token = Some(token);
                    },
                    Message::UdpAddressEvent(addr) => {
                        self.udp_address = Some(addr);
                        events.push(Event::Ready(self.ident.clone(), addr));
                    },
                    Message::LobbyCreateEvent(id) => {
                        // println!("[Client {}] Lobby created {}", self.ident, id);
                        // TODO lobbies updated
                    },
                    Message::LobbyDestroyEvent(id) => {
                        // println!("[Client {}] Lobby destroy {}", self.ident, id);
                        // TODO lobbies updated
                    },
                    Message::LobbyJoinEvent { id, addr, ident, is_local, is_owner } => {

                        if self.lobby.is_none() {
                            self.lobby = Some(Lobby {
                                id,
                                connections: Vec::new(),
                                preferences: HashMap::new()
                            });
                        }

                        // TODO de-duplicate updates?
                        if let Some(lobby) = self.lobby.as_mut() {
                            lobby.connections.push((ident, addr));
                            if is_local {
                                events.push(Event::LobbyJoined(lobby.id.clone(), lobby.connections.clone()));

                            } else {
                                events.push(Event::LobbyUpdated(lobby.id.clone(), lobby.connections.clone()));
                            }
                        }

                    },
                    // TODO public / private preferences
                    Message::LobbyPreferenceEvent(id, key, value) => {
                        if let Some(lobby) = self.lobby.as_mut() {
                            if lobby.id == id {
                                println!("[Client {}] Lobby pref {} {} => {}", self.ident, id, key, value);
                            }
                        }
                    },
                    Message::LobbyStartEvent(id) => {
                        if let Some(lobby) = self.lobby.as_mut() {
                            events.push(Event::LobbyStarted(
                                lobby.id.clone(),
                                self.udp_socket.try_clone().unwrap(),
                                lobby.connections.clone()
                            ));
                        }
                    },
                    Message::LobbyLeaveEvent(addr) => {
                        if let Some(lobby) = self.lobby.as_mut() {
                            if let Some(local_addr) = self.udp_address {
                                if local_addr == addr {
                                    events.push(Event::LobbyLeft(lobby.id.clone()));

                                } else {
                                    events.push(Event::LobbyUpdated(lobby.id.clone(), lobby.connections.clone()));
                                }
                            }
                        }
                    },
                    Message::InvalidLobby(id) => {

                    },
                    Message::InvalidAddress => {

                    },
                    Message::InvalidAction => {

                    },
                    _ => {}
                }
            },
            Err(_) => {}
        }

        if let Some(token) = self.udp_token {
            let buffer: [u8; 8] = [
                14,
                71,
                128,
                5,
                (token >> 24) as u8,
                (token >> 16) as u8,
                (token >> 8) as u8,
                (token) as u8
            ];
            self.udp_socket.send_to(&buffer, self.stream.peer_addr().unwrap()).ok();
        }

        events.into_iter()

    }

    fn receive(&mut self) -> Result<Vec<Message<LobbyId, LobbySecret, Identifier, Key, Value>>, ()> {
        let mut buffer: [u8; 255] = [0; 255];
        match self.stream.read(&mut buffer) {
            Ok(bytes) => {
                if bytes == 0 {
                    Err(())

                } else  {
                    let mut offset = 0;
                    let mut messages = Vec::new();
                    while let Ok(msg) = bincode::deserialize::<Message<LobbyId, LobbySecret, Identifier, Key, Value>>(&buffer[offset..]) {
                        offset += bincode::serialized_size::<Message<LobbyId, LobbySecret, Identifier, Key, Value>>(&msg).unwrap_or(0) as usize;
                        messages.push(msg);
                        if offset >= bytes {
                            break;
                        }
                    }
                    Ok(messages)
                }
            },
            Err(err) => {
                if err.kind() != ErrorKind::WouldBlock {
                    Err(())

                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    fn send(&mut self, msg: Message<LobbyId, LobbySecret, Identifier, Key, Value>) {
        let msg = bincode::serialize(&msg).unwrap();
        self.stream.write_all(&msg).ok();
    }

    fn close(&mut self) {

    }

}

pub struct Lobby<LobbyId: NetworkProperty, Identifier: NetworkProperty, Key: NetworkProperty, Value: NetworkProperty> {
    id: LobbyId,
    connections: Vec<(Identifier, SocketAddr)>,
    preferences: HashMap<Key, Value>
    // actions: Vec<Message<LobbyId, Key, Value>>
}

impl<
    LobbyId: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty

> Lobby<LobbyId, Identifier, Key, Value> {

    pub fn id(&self) -> LobbyId {
        self.id.clone()
    }

    pub fn set_preference(&mut self, key: Key, value: Value) {

    }

    pub fn preferences(&self) -> &HashMap<Key, Value> {
        &self.preferences
    }

    pub fn start(&mut self) {

    }

    pub fn leave(&mut self) {

    }

}

