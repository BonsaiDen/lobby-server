// Copyright (c) 2018 Ivo Wetzel

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


// STD Dependencies -----------------------------------------------------------
use std::collections::HashMap;
use std::io::{Error, ErrorKind, Read, Write};
use std::net::{UdpSocket, TcpStream, ToSocketAddrs};


// External Dependencies ------------------------------------------------------
use bincode;


// Internal Dependencies ------------------------------------------------------
use ::{ClientAction, Event, NetworkConfig, Message, UdpToken, UdpAddress, ServerEvent};


#[derive(Copy, Clone, Eq, PartialEq)]
enum ClientState {
    Connecting,
    Connected,
    Disconnecting,
    Disconnected
}


// Client Implementation ------------------------------------------------------
pub struct Client<C: NetworkConfig> {
    stream: TcpStream,
    state: ClientState,
    lobby: Option<Lobby<C>>,
    udp_socket: UdpSocket,
    udp_token: Option<UdpToken>,
    udp_address: Option<UdpAddress>,
    ident: C::ConnectionIdentifier
    // lobbies: HashMap<LobbyId, Lobby<LobbyId, Key, Value>>,
}

impl<C: NetworkConfig> Client<C> {

    pub fn new<A: ToSocketAddrs>(address: A, ident: C::ConnectionIdentifier) -> Result<Self, Error> {
        let stream = TcpStream::connect(address)?;
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            lobby: None,
            state: ClientState::Connecting,
            udp_socket: UdpSocket::bind("0.0.0.0:0")?,
            udp_token: None,
            udp_address: None,
            ident
            // lobbies: HashMap::new(),
        })
    }

    pub fn identify(&mut self, ident: C::ConnectionIdentifier) -> bool {
        self.send(&ClientAction::Identify(ident))
    }

    pub fn lobby_create(&mut self, id: C::LobbyId) -> bool {
        if self.lobby.is_none() {
            self.send(&ClientAction::LobbyCreate(id))

        } else {
            false
        }
    }

    pub fn lobby_join(&mut self, id: C::LobbyId, payload: Option<C::LobbyPayload>) -> bool {
        if self.lobby.is_none() {
            self.send(&ClientAction::LobbyJoin(id, payload))

        } else {
            false
        }
    }

    pub fn lobby_allow_join(&mut self, conn: Connection<C>) -> bool {
        if self.lobby.is_some() {
            self.send(&ClientAction::LobbyJoinResponse(conn.addr, true))

        } else {
            false
        }
    }

    pub fn lobby_deny_join(&mut self, conn: Connection<C>) -> bool {
        if self.lobby.is_some() {
            self.send(&ClientAction::LobbyJoinResponse(conn.addr, false))

        } else {
            false
        }
    }

    pub fn lobby_set_preference(&mut self, key: C::PreferenceKey, value: C::PreferenceValue) -> bool {
        if self.lobby.is_some() {
            self.send(&ClientAction::LobbyPreference {
                key,
                value,
                is_public: false
            })

        } else {
            false
        }
    }

    pub fn lobby_start(&mut self) -> bool {
        let addr = self.udp_address.clone();
        let is_owner = if let Some(lobby) = self.lobby.as_mut() {
            lobby.connections.iter().any(|c| c.is_owner && Some(c.addr) == addr)

        } else {
            false
        };
        if is_owner {
            self.send(&ClientAction::LobbyStart)

        } else{
            false
        }
    }

    pub fn lobby_leave(&mut self) -> bool {
        if self.lobby.is_none() {
            self.send(&ClientAction::LobbyLeave)

        } else {
            true
        }
    }

    pub fn events(&mut self) -> impl Iterator<Item = Event<C>> {

        let mut events = Vec::new();
        if self.state == ClientState::Connecting {
            events.push(Event::Connected);
            self.state = ClientState::Connected;

        } else if self.state == ClientState::Disconnecting {
            events.push(Event::Disconnected);
            self.state = ClientState::Disconnected;
        }

        match self.receive() {
            Ok(received) => for e in received {
                match e {
                    ServerEvent::Identify(token) => {
                        self.udp_token = Some(token);
                    },
                    ServerEvent::UdpAddress(addr) => {
                        self.udp_address = Some(addr);
                        events.push(Event::Ready(self.ident.clone(), addr));
                    },
                    ServerEvent::LobbyCreate(_) => {
                        // println!("[Client {}] Lobby created {}", self.ident, id);
                        // TODO lobbies updated
                    },
                    ServerEvent::LobbyDestroy(_) => {
                        // println!("[Client {}] Lobby destroy {}", self.ident, id);
                        // TODO lobbies updated
                    },
                    ServerEvent::LobbyJoinRequest { id, ident, addr, payload } => {
                        events.push(Event::LobbyJoinRequest(id, Connection {
                            ident,
                            addr,
                            is_owner: false,
                            is_local: false

                        }, payload));
                    },
                    ServerEvent::LobbyJoin { id, addr, ident, is_owner } => {
                        if let Some(local_addr) = self.udp_address {
                            if self.lobby.is_none() {
                                self.lobby = Some(Lobby {
                                    id,
                                    connections: Vec::new(),
                                    preferences: HashMap::new()
                                });
                            }

                            // TODO de-duplicate updates?
                            let is_local = self.is_local(addr);
                            if let Some(lobby) = self.lobby.as_mut() {
                                lobby.connections.push(Connection {
                                    ident,
                                    addr,
                                    is_local,
                                    is_owner
                                });
                                if addr == local_addr {
                                    events.push(Event::LobbyJoined(lobby.id.clone(), lobby.connections.clone(), lobby.preferences.clone()));

                                } else {
                                    events.push(Event::LobbyUpdated(lobby.id.clone(), lobby.connections.clone(), lobby.preferences.clone()));
                                }
                            }
                        }
                    },
                    ServerEvent::LobbyPreference { id, key, value } => {
                        // TODO public preferences on other lobbies
                        if let Some(lobby) = self.lobby.as_mut() {
                            if lobby.id == id {
                                lobby.preferences.insert(key, value);
                                events.push(Event::LobbyUpdated(lobby.id.clone(), lobby.connections.clone(), lobby.preferences.clone()));
                            }
                        }
                    },
                    ServerEvent::LobbyPreferenceRequest { id, ident, addr, key, value } => {
                        if let Some(lobby) = self.lobby.as_mut() {
                            if lobby.id == id {
                                events.push(Event::LobbyPreferenceRequest(id, Connection {
                                    ident,
                                    addr,
                                    is_local: false,
                                    is_owner: false

                                }, key, value));
                            }
                        }
                    },
                    ServerEvent::LobbyStart(_) => {
                        if let Some(lobby) = self.lobby.take() {
                            events.push(Event::LobbyStarted(
                                lobby.id,
                                self.udp_socket.try_clone().unwrap(),
                                lobby.connections,
                                lobby.preferences
                            ));
                        }
                        // TODO remove from lobby list
                    },
                    ServerEvent::LobbyLeave(addr) => {
                        let is_local = self.is_local(addr);
                        let left = if let Some(lobby) = self.lobby.as_mut() {
                            lobby.connections.retain(|conn| conn.addr != addr);
                            if is_local {
                                events.push(Event::LobbyLeft(lobby.id.clone()));
                                true

                            } else {
                                events.push(Event::LobbyUpdated(lobby.id.clone(), lobby.connections.clone(), lobby.preferences.clone()));
                                true
                            }

                        } else {
                            false
                        };

                        if left {
                            self.lobby = None;
                        }

                    },
                    ServerEvent::Error(err) => {
                        events.push(Event::Error(err))
                    }
                }
            },
            Err(_) => {}
        }

        if let Some(token) = self.udp_token {
            let buffer = token.into_buffer();
            self.udp_socket.send_to(&buffer, self.stream.peer_addr().unwrap()).ok();
        }

        events.into_iter()

    }

    pub fn close(&mut self) {
        if self.state == ClientState::Connected {
            self.state = ClientState::Disconnecting;
        }
    }

    fn is_local(&self, addr: UdpAddress) -> bool {
        if let Some(local_addr) = self.udp_address {
            local_addr == addr

        } else {
            false
        }
    }

    fn receive(&mut self) -> Result<Vec<ServerEvent<C>>, ()> {
        let mut buffer: [u8; 255] = [0; 255];
        match self.stream.read(&mut buffer) {
            Ok(bytes) => {
                if bytes == 0 {
                    self.close();
                    Err(())

                } else  {
                    let mut offset = 0;
                    let mut events = Vec::new();
                    while let Ok(msg) = bincode::deserialize::<Message<C>>(&buffer[offset..]) {
                        offset += bincode::serialized_size::<Message<C>>(&msg).unwrap_or(0) as usize;
                        if let Message::ServerEvent(event) = msg {
                            events.push(event);
                        }
                        if offset >= bytes {
                            break;
                        }
                    }
                    Ok(events)
                }
            },
            Err(err) => {
                if err.kind() != ErrorKind::WouldBlock {
                    self.close();
                    Err(())

                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    fn send(&mut self, msg: &ClientAction<C>) -> bool {
        if self.state == ClientState::Connected {
            let msg = Message::ClientAction(msg.clone());
            let msg = bincode::serialize(&msg).unwrap();
            self.stream.write_all(&msg).ok();
            true

        } else {
            false
        }
    }

}

#[derive(Debug, Clone)]
pub struct Connection<C: NetworkConfig> {
    pub ident: C::ConnectionIdentifier,
    pub addr: UdpAddress,
    pub is_local: bool,
    pub is_owner: bool
}

struct Lobby<C: NetworkConfig> {
    id: C::LobbyId,
    connections: Vec<Connection<C>>,
    preferences: HashMap<C::PreferenceKey, C::PreferenceValue>
}

