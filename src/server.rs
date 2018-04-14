// Copyright (c) 2018 Ivo Wetzel

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


// STD Dependencies -----------------------------------------------------------
use std::fmt;
use std::thread;
use std::marker::PhantomData;
use std::collections::HashMap;
use std::io::{Error, ErrorKind, Read, Write};
use std::time::{Duration, Instant};
use std::net::{UdpSocket, TcpListener, TcpStream, ToSocketAddrs, SocketAddr};


// External Dependencies ------------------------------------------------------
use rand;
use rand::Rng;
use bincode;


// Internal Dependencies ------------------------------------------------------
use ::{NetworkProperty, Message};

type UdpToken = u32;


// Server Implementation ------------------------------------------------------
pub struct Server<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty
> {
    tick_rate: u64,
    lobbies: HashMap<LobbyId, Lobby<LobbyId, LobbySecret, Identifier, Key, Value>>,
    connections: HashMap<SocketAddr, Connection<LobbyId, LobbySecret, Identifier, Key, Value>>,
    tokens: HashMap<UdpToken, SocketAddr>,
    responses: Vec<Response<LobbyId, LobbySecret, Identifier, Key, Value>>,
    info: Instant
}

impl<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty

> Server<LobbyId, LobbySecret, Identifier, Key, Value> {

    pub fn new(tick_rate: u64) -> Self {
        Self {
            tick_rate,
            lobbies: HashMap::new(),
            connections: HashMap::new(),
            tokens: HashMap::new(),
            responses: Vec::new(),
            info: Instant::now()
        }
    }

    pub fn listen<A: ToSocketAddrs + Clone>(mut self, address: A) -> Result<(), Error> {

        let listener = TcpListener::bind(address.clone())?;
        listener.set_nonblocking(true)?;

        let udp_listener = UdpSocket::bind(address)?;
        udp_listener.set_nonblocking(true)?;

        println!("[Server] Started");

        loop {

            // TCP Control Messages
            while let Ok((stream, addr)) = listener.accept() {
                self.connect(stream, addr).ok();
            }

            // UDP Idenfification Messages
            for (conn, msg) in self.receive(&udp_listener) {
                let response = match msg {
                    Message::ReadyAction => {
                        println!("[Connection] {} identified", conn);
                        let mut r = vec![Server::connection_event(&conn, Message::IdentifyEvent(conn.token))];
                        for lobby in self.lobbies.values() {
                            r.push(lobby.info(&conn));
                        }
                        Response::Many(r)
                    },
                    Message::UdpAddressAction(addr) => {
                        println!("[Connection] {} assigned UDP address {}", conn, addr);
                        Server::connection_event(&conn, Message::UdpAddressEvent(addr))
                    },
                    Message::LobbyJoinAction { id, secret } => {
                        if conn.lobby_id.is_some() {
                            Server::invalid_action(&conn)

                        } else if conn.udp_address.is_none() {
                            Server::invalid_address(&conn)

                        } else if self.lobbies.contains_key(&id) {
                            self.join_lobby(&conn, id, secret)

                        } else {
                            self.create_lobby(&conn, id, secret)
                        }
                    },
                    Message::LobbyPreferenceAction(key, value, public) => match self.get_lobby_if_owner(&conn) {
                        Ok(lobby) => {
                            println!("[Lobby] \"{}\" preference \"{}\" set to \"{}\" by {}", lobby.id, key, value, conn);
                            lobby.preferences.insert(key.clone(), (value.clone(), public));
                            if public {
                                Server::public_event(Message::LobbyPreferenceEvent(lobby.id.clone(), key, value))

                            } else {
                                Server::lobby_event(lobby.id.clone(), Message::LobbyPreferenceEvent(lobby.id.clone(), key, value))
                            }
                        },
                        Err(event) => event
                    },
                    Message::LobbyLeaveAction => match self.get_lobby(&conn) {
                        Ok(lobby) => if lobby.owned_by(&conn) {
                            println!("[Lobby] \"{}\" closed by {}", lobby.id, conn);
                            lobby.close(&conn)

                        } else {
                            println!("[Lobby] \"{}\" left by {}", lobby.id, conn);
                            lobby.leave(&conn)
                        },
                        Err(event) => event
                    },
                    Message::LobbyStartAction => match self.get_lobby_if_owner(&conn) {
                        Ok(lobby) => {
                            println!("[Lobby] \"{}\" started by {}", lobby.id, conn);
                            lobby.start()
                        }
                        Err(event) => event
                    },
                    _ => Server::invalid_action(&conn)
                };
                self.responses.push(response);
            }

            self.lobbies.retain(|_, lobby| lobby.open);

            let responses: Vec<Response<LobbyId, LobbySecret, Identifier, Key, Value>> = self.responses.drain(0..).collect();
            for response in responses {
                self.send(response);
            }

            thread::sleep(Duration::from_millis(1000 / self.tick_rate));

            if self.info.elapsed() > Duration::from_millis(5000) {
                println!("[Server] {} connection(s) in {} lobby(s)", self.connections.len(), self.lobbies.len());
                self.info = Instant::now();
            }

        }

    }

    fn connect(&mut self, stream: TcpStream, addr: SocketAddr) -> Result<(), Error> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let conn = Connection::new(addr, stream);
        self.tokens.insert(conn.token, conn.addr);
        self.connections.insert(addr, conn);
        println!("[Connection] {} connected", addr);
        Ok(())
    }

    fn receive(&mut self, udp_listener: &UdpSocket) -> Vec<(ConnectionInfo<LobbyId, Identifier>, Message<LobbyId, LobbySecret, Identifier, Key, Value>)> {

        let mut messages = Vec::new();
        for conn in &mut self.connections.values_mut() {
            messages.append(&mut conn.receive());
        }

        let mut buffer: [u8; 8] = [0; 8];
        while let Ok((len, udp_addr)) = udp_listener.recv_from(&mut buffer) {
            if len == 8 && buffer[0] == 14 && buffer[1] == 71 && buffer[2] == 128 && buffer[3] == 5 {
                let token = ((buffer[4] as u32) << 24) | ((buffer[5] as u32) << 16) | ((buffer[6] as u32) << 8) | (buffer[7] as u32);
                if let Some(addr) = self.tokens.remove(&token) {
                    if let Some(conn) = self.connections.get_mut(&addr) {
                        messages.push(conn.set_udp_address(udp_addr));
                    }
                }
            }
        }

        self.connections.retain(|_, conn| conn.open);
        messages

    }

    fn send(&mut self, response: Response<LobbyId, LobbySecret, Identifier, Key, Value>) {
        match response {
            Response::Empty => {},
            Response::Single(recp, msg) => match recp {
                Recipient::Everyone => {
                    for conn in self.connections.values_mut() {
                        conn.send(msg.clone());
                    }
                },
                Recipient::Connection(addr) => {
                    if let Some(conn) = self.connections.get_mut(&addr) {
                        conn.send(msg);
                    }
                },
                Recipient::Lobby(id) => {
                    if let Some(lobby) = self.lobbies.get(&id) {
                        for conn in &lobby.connections {
                            if let Some(conn) = self.connections.get_mut(&conn.addr) {
                                conn.send(msg.clone());
                            }
                        }
                    }
                }
            },
            Response::Many(responses) => {
                for response in responses {
                    self.send(response);
                }
            }
        }
    }

    fn join_lobby(&mut self, conn: &ConnectionInfo<LobbyId, Identifier>, id: LobbyId, secret: Option<LobbySecret>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        if let Some(lobby) = self.lobbies.get_mut(&id) {
            if lobby.secret == secret {

                let mut r: Vec<Response<LobbyId, LobbySecret, Identifier, Key, Value>> = lobby.connections.iter().map(|c| {
                    Server::connection_event(conn, Message::LobbyJoinEvent {
                        addr: c.udp_address.expect("Connection without UDP address in lobby"),
                        ident: c.ident.clone(),
                        is_local: false,
                        is_owner: lobby.owned_by(c)
                    })

                }).collect();

                lobby.connections.push(conn.clone());

                for (key, &(ref value, _)) in &lobby.preferences {
                    r.push(Server::connection_event(conn, Message::LobbyPreferenceEvent(lobby.id.clone(), key.clone(), value.clone())));
                }

                println!("[Lobby] \"{}\" joined by {}", id, conn);

                r.push(Server::lobby_event(id.clone(), Message::LobbyJoinEvent {
                    addr: conn.udp_address.expect("Connection without UDP joined lobby"),
                    ident: conn.ident.clone(),
                    is_local: true,
                    is_owner: false
                }));
                Response::Many(r)

            } else {
                Server::connection_event(conn, Message::InvalidLobbySecret(lobby.id.clone()))
            }

        } else {
            Response::Empty
        }
    }

    fn create_lobby(&mut self, conn: &ConnectionInfo<LobbyId, Identifier>, id: LobbyId, secret: Option<LobbySecret>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {

        self.lobbies.insert(id.clone(), Lobby {
            id: id.clone(),
            open: true,
            owner: conn.clone(),
            secret,
            connections: vec![conn.clone()],
            preferences: HashMap::new(),
            _ident: PhantomData
        });

        println!("[Lobby] \"{}\" created by {}", id, conn);

        Response::Many(vec![
            Server::public_event(Message::LobbyCreateEvent(id.clone())),
            Server::connection_event(conn, Message::LobbyJoinEvent {
                addr: conn.udp_address.expect("Connection without UDP created lobby"),
                ident: conn.ident.clone(),
                is_local: true,
                is_owner: true
            })
        ])

    }

    fn get_lobby(&mut self, conn: &ConnectionInfo<LobbyId, Identifier>) -> Result<&mut Lobby<LobbyId, LobbySecret, Identifier, Key, Value>, Response<LobbyId, LobbySecret, Identifier, Key, Value>> {
        if conn.udp_address.is_none() {
            Err(Server::invalid_address(conn))

        } else if let Some(ref id) = conn.lobby_id {
            if let Some(lobby) = self.lobbies.get_mut(&id) {
                Ok(lobby)

            // Lobby not found
            } else {
                Err(Server::invalid_lobby(conn, id.clone()))
            }

        // Not in any Lobby
        } else {
            Err(Server::invalid_action(conn))
        }
    }

    fn get_lobby_if_owner(&mut self, conn: &ConnectionInfo<LobbyId, Identifier>) -> Result<&mut Lobby<LobbyId, LobbySecret, Identifier, Key, Value>, Response<LobbyId, LobbySecret, Identifier, Key, Value>> {
        if conn.udp_address.is_none() {
            Err(Server::invalid_address(conn))

        } else if let Some(ref id) = conn.lobby_id {
            if let Some(lobby) = self.lobbies.get_mut(&id) {
                if lobby.owned_by(&conn) {
                    Ok(lobby)

                // Not owner
                } else {
                    Err(Server::invalid_action(conn))
                }

            // Lobby not found
            } else {
                Err(Server::invalid_lobby(conn, id.clone()))
            }

        // Not in any Lobby
        } else {
            Err(Server::invalid_action(conn))
        }
    }

    fn connection_event(conn: &ConnectionInfo<LobbyId, Identifier>, event: Message<LobbyId, LobbySecret, Identifier, Key, Value>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        Response::Single(Recipient::Connection(conn.addr), event)
    }

    fn lobby_event(id: LobbyId, event: Message<LobbyId, LobbySecret, Identifier, Key, Value>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        Response::Single(Recipient::Lobby(id), event)
    }

    fn public_event(event: Message<LobbyId, LobbySecret, Identifier, Key, Value>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        Response::Single(Recipient::Everyone, event)
    }

    fn invalid_address(conn: &ConnectionInfo<LobbyId, Identifier>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        Response::Single(Recipient::Connection(conn.addr), Message::InvalidAddress)
    }

    fn invalid_action(conn: &ConnectionInfo<LobbyId, Identifier>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        Response::Single(Recipient::Connection(conn.addr), Message::InvalidAction)
    }

    fn invalid_lobby(conn: &ConnectionInfo<LobbyId, Identifier>, id: LobbyId) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        Response::Single(Recipient::Connection(conn.addr), Message::InvalidLobby(id))
    }

}

enum Response<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty
> {
    Empty,
    Single(Recipient<LobbyId>, Message<LobbyId, LobbySecret, Identifier, Key, Value>),
    Many(Vec<Response<LobbyId, LobbySecret, Identifier, Key, Value>>)
}

enum Recipient<LobbyId: NetworkProperty> {
    Everyone,
    Connection(SocketAddr),
    Lobby(LobbyId)
}

#[derive(Clone)]
struct ConnectionInfo<LobbyId: NetworkProperty, Identifier: NetworkProperty> {
    addr: SocketAddr,
    udp_address: Option<SocketAddr>,
    lobby_id: Option<LobbyId>,
    ident: Identifier,
    token: UdpToken
}

impl<LobbyId: NetworkProperty, Identifier: NetworkProperty> fmt::Display for ConnectionInfo<LobbyId, Identifier> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<\"{}\"@{}@{:?}>", self.ident, self.addr, self.udp_address)
    }
}

struct Connection<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty
> {
    lobby_id: Option<LobbyId>,
    open: bool,
    addr: SocketAddr,
    udp_address: Option<SocketAddr>,
    stream: TcpStream,
    token: UdpToken,
    ident: Option<Identifier>,
    _secret: PhantomData<LobbySecret>,
    _key: PhantomData<Key>,
    _value: PhantomData<Value>
}

impl<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty

> Connection<LobbyId, LobbySecret, Identifier, Key, Value> {

    fn new(addr: SocketAddr, stream: TcpStream) -> Self {
        Self {
            lobby_id: None,
            open: true,
            addr,
            udp_address: None,
            stream,
            token: rand::thread_rng().next_u32(),
            ident: None,
            _secret: PhantomData,
            _key: PhantomData,
            _value: PhantomData
        }
    }

    fn info(&self) -> ConnectionInfo<LobbyId, Identifier> {
        ConnectionInfo {
            addr: self.addr,
            lobby_id: self.lobby_id.clone(),
            udp_address: self.udp_address.clone(),
            token: self.token,
            ident: self.ident.clone().expect("Message forwarded before identification")
        }
    }

    fn set_udp_address(&mut self, addr: SocketAddr) -> (ConnectionInfo<LobbyId, Identifier>, Message<LobbyId, LobbySecret, Identifier, Key, Value>) {
        self.udp_address = Some(addr);
        (self.info(), Message::UdpAddressAction(addr))
    }

    fn receive(&mut self) -> Vec<(ConnectionInfo<LobbyId, Identifier>, Message<LobbyId, LobbySecret, Identifier, Key, Value>)> {
        let mut buffer: [u8; 255] = [0; 255];
        let mut messages = Vec::new();
        match self.stream.read(&mut buffer) {
            Ok(bytes) => {
                if bytes == 0 {
                    self.close();
                    messages.push((self.info(), Message::LobbyLeaveAction));

                } else {
                    let mut offset = 0;
                    while let Ok(msg) = bincode::deserialize::<Message<LobbyId, LobbySecret, Identifier, Key, Value>>(&buffer[offset..]) {

                        offset += bincode::serialized_size::<Message<LobbyId, LobbySecret, Identifier, Key, Value>>(&msg).unwrap_or(0) as usize;

                        if self.ident.is_some() {
                            messages.push((self.info(), msg));

                        // Wait for identification
                        } else if let Message::IdentifyAction(ident) = msg {
                            self.ident = Some(ident);
                            messages.push((self.info(), Message::ReadyAction));
                        }

                        if offset >= bytes {
                            break;
                        }

                    }
                }
            },
            Err(err) => if err.kind() != ErrorKind::WouldBlock {
                self.close();
                messages.push((self.info(), Message::LobbyLeaveAction));
            }
        };
        messages
    }

    fn send(&mut self, msg: Message<LobbyId, LobbySecret, Identifier, Key, Value>) {
        let bytes = bincode::serialize(&msg).unwrap();
        self.stream.write_all(&bytes[..]).ok();
    }

    fn close(&mut self) {
        println!("[Connection] {} left", self.addr);
        self.open = false;
    }

}

struct Lobby<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty
> {
    id: LobbyId,
    open: bool,
    owner: ConnectionInfo<LobbyId, Identifier>,
    secret: Option<LobbySecret>,
    connections: Vec<ConnectionInfo<LobbyId, Identifier>>,
    preferences: HashMap<Key, (Value, bool)>,
    _ident: PhantomData<Identifier>
}

impl<
    LobbyId: NetworkProperty,
    LobbySecret: NetworkProperty,
    Identifier: NetworkProperty,
    Key: NetworkProperty,
    Value: NetworkProperty

> Lobby<LobbyId, LobbySecret, Identifier, Key, Value> {

    fn owned_by(&self, conn: &ConnectionInfo<LobbyId, Identifier>) -> bool {
        self.owner.addr == conn.addr
    }

    fn info(&self, conn: &ConnectionInfo<LobbyId, Identifier>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        let mut r = vec![
            Server::connection_event(conn, Message::LobbyCreateEvent(self.id.clone()))
        ];
        for (key, &(ref value, public)) in &self.preferences {
            if public {
                r.push(Server::connection_event(conn, Message::LobbyPreferenceEvent(self.id.clone(), key.clone(), value.clone())));
            }
        }
        Response::Many(r)
    }

    fn start(&mut self) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        self.open = false;
        Response::Many(self.connections.iter().map(|conn| {
            Server::connection_event(conn, Message::LobbyStartEvent(self.id.clone()))

        }).collect())
    }

    fn leave(&mut self, conn: &ConnectionInfo<LobbyId, Identifier>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        self.connections.retain(|c| c.addr != conn.addr);
        Response::Many(vec![
            Server::connection_event(conn, Message::LobbyLeaveEvent(conn.addr)),
            Server::lobby_event(self.id.clone(), Message::LobbyLeaveEvent(conn.addr))
        ])
    }

    fn close(&mut self, conn: &ConnectionInfo<LobbyId, Identifier>) -> Response<LobbyId, LobbySecret, Identifier, Key, Value> {
        self.open = false;
        let mut r: Vec<Response<LobbyId, LobbySecret, Identifier, Key, Value>> = self.connections.iter().map(|c| {
            Server::connection_event(conn, Message::LobbyLeaveEvent(c.addr))

        }).collect();
        r.push(Server::public_event(Message::LobbyDestroyEvent(self.id.clone())));
        Response::Many(r)
    }

}

