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


// Client Implementation ------------------------------------------------------
pub struct Client<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty
>{
    stream: TcpStream,
    lobby: Option<Lobby<LobbyId, Key, Value>>,
    lobbies: HashMap<LobbyId, Lobby<LobbyId, Key, Value>>,
    udp_socket: UdpSocket,
    address_token: Option<u32>,
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
            lobbies: HashMap::new(),
            udp_socket: UdpSocket::bind("0.0.0.0:0")?,
            address_token: None,
            ident,
            _secret: PhantomData
        })
    }

    pub fn identify(&mut self, ident: Identifier) {
        self.send(Message::IdentifyAction(ident));
    }

    pub fn create_lobby(&mut self, id: LobbyId) {
        self.send(Message::LobbyJoinAction {
            id,
            secret: None
        });
    }

    fn send(&mut self, msg: Message<LobbyId, LobbySecret, Identifier, Key, Value>) {
        let msg = bincode::serialize(&msg).unwrap();
        self.stream.write_all(&msg).ok();
    }

    pub fn join_lobby(&mut self, id: LobbyId) {
        // TODO create local udp socket
    }

    pub fn lobby(&self) -> Option<&mut Lobby<LobbyId, Key, Value>> {
        None
    }

    pub fn events(&mut self) {
        match self.receive() {
            Ok(messages) => for m in messages {
                match m {
                    Message::IdentifyEvent(token) => {
                        println!("[Client {}] Token {}", self.ident, token);
                        self.address_token = Some(token);
                    },
                    Message::UdpAddressEvent(addr) => {
                        println!("[Client {}] External UDP address {}", self.ident, addr);
                        // self.address_token = None;
                    },
                    Message::LobbyCreateEvent(id) => {
                        println!("[Client {}] Lobby created {}", self.ident, id);
                        // TODO lobbies updated
                    },
                    Message::LobbyDestroyEvent(id) => {
                        println!("[Client {}] Lobby destroy {}", self.ident, id);
                        // TODO lobbies updated
                    },
                    Message::LobbyJoinEvent { addr, ident, is_local, is_owner } => {
                        println!("[Client {}] Lobby joined {} {} {:?} {}", self.ident, addr, ident, is_local, is_owner);
                    },
                    // TODO public / private preferences
                    Message::LobbyPreferenceEvent(id, key, value) => {
                        println!("[Client {}] Lobby pref {} {} => {}", self.ident, id, key, value);
                    },
                    Message::LobbyStartEvent(id) => {

                    },
                    Message::LobbyLeaveEvent(addr) => {
                        println!("[Client {}] Lobby left {}", self.ident, addr);
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

        if let Some(token) = self.address_token {
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

    fn close(&mut self) {

    }

}

pub struct Lobby<LobbyId: NetworkProperty, Key: NetworkProperty, Value: NetworkProperty> {
    id: LobbyId,
    connections: HashMap<SocketAddr, u16>,
    preferences: HashMap<Key, Value>,
    stream: Option<TcpStream>,
    // actions: Vec<Message<LobbyId, Key, Value>>
}

impl<LobbyId: NetworkProperty, Key: NetworkProperty, Value: NetworkProperty> Lobby<LobbyId, Key, Value> {

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

