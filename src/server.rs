// Copyright (c) 2018 Ivo Wetzel

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


// STD Dependencies -----------------------------------------------------------
use std::fmt;
use std::thread;
use std::collections::HashMap;
use std::io::{Error, ErrorKind, Read, Write};
use std::time::{Duration, Instant};
use std::net::{UdpSocket, TcpListener, TcpStream, ToSocketAddrs, SocketAddr};


// External Dependencies ------------------------------------------------------
use rand;
use rand::Rng;
use bincode;


// Internal Dependencies ------------------------------------------------------
use ::{NetworkConfig, Message};


// Type Aliases ---------------------------------------------------------------
type UdpToken = u32;

// Server Implementation ------------------------------------------------------
pub struct Server<C: NetworkConfig> {
    tick_rate: u64,
    lobbies: HashMap<C::LobbyId, Lobby<C>>,
    connections: HashMap<SocketAddr, Connection<C>>,
    tokens: HashMap<UdpToken, SocketAddr>,
    responses: Vec<Response<C>>,
    info: Instant
}

impl<C: NetworkConfig> Server<C> {

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

        info!("[Server] Started");

        loop {

            // TCP Control Messages
            while let Ok((stream, addr)) = listener.accept() {
                self.connect(stream, addr).ok();
            }

            // UDP Idenfification Messages
            for (conn, msg) in self.receive(&udp_listener) {
                let response = match msg {
                    Message::ReadyAction => {
                        info!("[Connection] {} identified", conn);
                        let mut r = vec![Server::connection_event(&conn, Message::IdentifyEvent(conn.token))];
                        for lobby in self.lobbies.values() {
                            r.push(lobby.info(&conn));
                        }
                        Response::Many(r)
                    },
                    Message::UdpAddressAction(addr) => {
                        info!("[Connection] {} assigned UDP address {}", conn, addr);
                        Server::connection_event(&conn, Message::UdpAddressEvent(addr))
                    },
                    Message::LobbyCreateAction(id) => {
                        if conn.lobby_id.is_some() {
                            Server::invalid_action(&conn)

                        } else if conn.udp_address.is_none() {
                            Server::invalid_address(&conn)

                        } else if self.lobbies.contains_key(&id) {
                            Server::invalid_action(&conn)

                        } else {
                            self.set_lobby(&conn, Some(id.clone()));
                            self.create_lobby(&conn, id)
                        }
                    },
                    Message::LobbyJoinAction(id, payload) => {
                        if conn.lobby_id.is_some() {
                            Server::invalid_action(&conn)

                        } else if conn.udp_address.is_none() {
                            Server::invalid_address(&conn)

                        } else if let Some(lobby) = self.lobbies.get_mut(&id) {
                            Server::connection_event(&lobby.owner, Message::LobbyJoinRequestEvent {
                                id: lobby.id.clone(),
                                ident: conn.ident.clone(),
                                addr: conn.udp_address.unwrap(),
                                payload
                            })

                        } else {
                            Server::invalid_action(&conn)
                        }
                    },
                    Message::LobbyJoinResponseAction(addr, allow) => {
                        if let Some(id) = conn.lobby_id.clone() {
                            let join_conn = self.connections.values().find(|c| c.udp_address == Some(addr)).map(|c| c.info());
                            if let Some(conn) = join_conn {
                                if allow {
                                    info!("[Lobby] \"{}\" allowed join for {}", id, conn);
                                    self.set_lobby(&conn, Some(id.clone()));
                                    self.join_lobby(&conn, id)

                                } else {
                                    info!("[Lobby] \"{}\" denied join for {}", id, conn);
                                    Server::invalid_lobby(&conn, id)
                                }

                            } else {
                                Server::invalid_action(&conn)
                            }

                        } else {
                            Server::invalid_action(&conn)
                        }
                    },
                    Message::LobbyPreferenceAction(key, value, public) => match self.get_lobby(&conn) {
                        Ok(lobby) => if lobby.owned_by(&conn) {
                            info!("[Lobby] \"{}\" preference \"{}\" set to \"{}\" by {}", lobby.id, key, value, conn);
                            lobby.preferences.insert(key.clone(), (value.clone(), public));
                            if public {
                                Server::public_event(Message::LobbyPreferenceEvent(lobby.id.clone(), key, value))

                            } else {
                                Server::lobby_event(lobby.id.clone(), Message::LobbyPreferenceEvent(lobby.id.clone(), key, value))
                            }

                        } else {
                            Server::connection_event(&lobby.owner, Message::LobbyPreferenceRequestEvent(
                                lobby.id.clone(),
                                conn.ident.clone(),
                                conn.udp_address.unwrap(),
                                key,
                                value
                            ))
                        },
                        Err(event) => event
                    },
                    Message::LobbyLeaveAction => {
                        let (left, event) = match self.get_lobby(&conn) {
                            Ok(lobby) => if lobby.owned_by(&conn) {
                                info!("[Lobby] \"{}\" closed by {}", lobby.id, conn);
                                let left = lobby.connections.clone();
                                (left, lobby.close())

                            } else {
                                info!("[Lobby] \"{}\" left by {}", lobby.id, conn);
                                (vec![conn.clone()], lobby.leave(&conn))
                            },
                            Err(event) => {
                                (Vec::new(), event)
                            }
                        };
                        for conn in left {
                            self.set_lobby(&conn, None);
                        }
                        event
                    }
                    Message::LobbyStartAction => match self.get_lobby_if_owner(&conn) {
                        Ok(lobby) => {
                            info!("[Lobby] \"{}\" started by {}", lobby.id, conn);
                            lobby.start()
                        }
                        Err(event) => event
                    },
                    _ => Server::invalid_action(&conn)
                };
                self.responses.push(response);
            }

            self.lobbies.retain(|_, lobby| lobby.open);

            let responses: Vec<Response<C>> = self.responses.drain(0..).collect();
            for response in responses {
                self.send(response);
            }

            thread::sleep(Duration::from_millis(1000 / self.tick_rate));

            if self.info.elapsed() > Duration::from_millis(5000) {
                info!("[Server] {} connection(s) in {} lobby(s)", self.connections.len(), self.lobbies.len());
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
        info!("[Connection] {} connected", addr);
        Ok(())
    }

    fn receive(&mut self, udp_listener: &UdpSocket) -> Vec<(ConnectionInfo<C>, Message<C>)> {

        let mut messages = Vec::new();
        for conn in &mut self.connections.values_mut() {
            messages.append(&mut conn.receive());
        }

        let mut buffer: [u8; 8] = [0; 8];
        while let Ok((len, udp_addr)) = udp_listener.recv_from(&mut buffer) {
            if len == 8 && buffer[0] == 14 && buffer[1] == 71 && buffer[2] == 128 && buffer[3] == 5 {
                let token = (u32::from(buffer[4]) << 24)
                          | (u32::from(buffer[5]) << 16)
                          | (u32::from(buffer[6]) << 8)
                          | u32::from(buffer[7] );

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

    fn send(&mut self, response: Response<C>) {
        match response {
            Response::Empty => {},
            Response::Single(recp, msg) => match recp {
                Recipient::Everyone => {
                    for conn in self.connections.values_mut() {
                        conn.send(&msg);
                    }
                },
                Recipient::Connection(addr) => {
                    if let Some(conn) = self.connections.get_mut(&addr) {
                        conn.send(&msg);
                    }
                },
                Recipient::Lobby(id) => {
                    if let Some(lobby) = self.lobbies.get(&id) {
                        for conn in &lobby.connections {
                            if let Some(conn) = self.connections.get_mut(&conn.tcp_addr) {
                                conn.send(&msg);
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

    fn set_lobby(&mut self, conn: &ConnectionInfo<C>, id: Option<C::LobbyId>) {
        if let Some(conn) = self.connections.get_mut(&conn.tcp_addr) {
            conn.lobby_id = id;
        }
    }

    fn join_lobby(&mut self, conn: &ConnectionInfo<C>, id: C::LobbyId) -> Response<C> {
        if let Some(lobby) = self.lobbies.get_mut(&id) {

            let mut r: Vec<Response<C>> = vec![
                Server::lobby_event(id.clone(), Message::LobbyJoinEvent {
                    id: id.clone(),
                    addr: conn.udp_address.expect("Connection without UDP joined lobby"),
                    ident: conn.ident.clone(),
                    is_owner: false
                })
            ];

            for c in &lobby.connections {
                r.push(Server::connection_event(conn, Message::LobbyJoinEvent {
                    id: id.clone(),
                    addr: c.udp_address.expect("Connection without UDP address in lobby"),
                    ident: c.ident.clone(),
                    is_owner: lobby.owned_by(c)
                }));
            }

            for (key, &(ref value, _)) in &lobby.preferences {
                r.push(Server::connection_event(conn, Message::LobbyPreferenceEvent(lobby.id.clone(), key.clone(), value.clone())));
            }

            lobby.connections.push(conn.clone());

            info!("[Lobby] \"{}\" joined by {}", id, conn);
            Response::Many(r)

        } else {
            Response::Empty
        }
    }

    fn create_lobby(&mut self, conn: &ConnectionInfo<C>, id: C::LobbyId) -> Response<C> {

        self.lobbies.insert(id.clone(), Lobby {
            id: id.clone(),
            open: true,
            owner: conn.clone(),
            connections: vec![conn.clone()],
            preferences: HashMap::new()
        });

        info!("[Lobby] \"{}\" created by {}", id, conn);

        Response::Many(vec![
            Server::public_event(Message::LobbyCreateEvent(id.clone())),
            Server::connection_event(conn, Message::LobbyJoinEvent {
                id,
                addr: conn.udp_address.expect("Connection without UDP created lobby"),
                ident: conn.ident.clone(),
                is_owner: true
            })
        ])

    }

    fn get_lobby(&mut self, conn: &ConnectionInfo<C>) -> Result<&mut Lobby<C>, Response<C>> {
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

    fn get_lobby_if_owner(&mut self, conn: &ConnectionInfo<C>) -> Result<&mut Lobby<C>, Response<C>> {
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

    fn connection_event(conn: &ConnectionInfo<C>, event: Message<C>) -> Response<C> {
        Response::Single(Recipient::Connection(conn.tcp_addr), event)
    }

    fn lobby_event(id: C::LobbyId, event: Message<C>) -> Response<C> {
        Response::Single(Recipient::Lobby(id), event)
    }

    fn public_event(event: Message<C>) -> Response<C> {
        Response::Single(Recipient::Everyone, event)
    }

    fn invalid_address(conn: &ConnectionInfo<C>) -> Response<C> {
        Response::Single(Recipient::Connection(conn.tcp_addr), Message::InvalidAddress)
    }

    fn invalid_action(conn: &ConnectionInfo<C>) -> Response<C> {
        Response::Single(Recipient::Connection(conn.tcp_addr), Message::InvalidAction)
    }

    fn invalid_lobby(conn: &ConnectionInfo<C>, id: C::LobbyId) -> Response<C> {
        Response::Single(Recipient::Connection(conn.tcp_addr), Message::InvalidLobby(id))
    }

}

enum Response<C: NetworkConfig> {
    Empty,
    Single(Recipient<C>, Message<C>),
    Many(Vec<Response<C>>)
}

enum Recipient<C: NetworkConfig> {
    Everyone,
    Connection(SocketAddr),
    Lobby(C::LobbyId)
}

#[derive(Clone)]
struct ConnectionInfo<C: NetworkConfig> {
    tcp_addr: SocketAddr,
    udp_address: Option<SocketAddr>,
    lobby_id: Option<C::LobbyId>,
    ident: C::ConnectionIdentifier,
    token: UdpToken
}

impl<C: NetworkConfig> fmt::Display for ConnectionInfo<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<\"{}\"@{}@{:?}>", self.ident, self.tcp_addr, self.udp_address)
    }
}

struct Connection<C: NetworkConfig> {
    lobby_id: Option<C::LobbyId>,
    open: bool,
    addr: SocketAddr,
    udp_address: Option<SocketAddr>,
    stream: TcpStream,
    token: UdpToken,
    ident: Option<C::ConnectionIdentifier>
}

impl<C: NetworkConfig> Connection<C> {

    fn new(addr: SocketAddr, stream: TcpStream) -> Self {
        Self {
            lobby_id: None,
            open: true,
            addr,
            udp_address: None,
            stream,
            token: rand::thread_rng().next_u32(),
            ident: None
        }
    }

    fn info(&self) -> ConnectionInfo<C> {
        ConnectionInfo {
            tcp_addr: self.addr,
            lobby_id: self.lobby_id.clone(),
            udp_address: self.udp_address,
            token: self.token,
            ident: self.ident.clone().expect("Message forwarded before identification")
        }
    }

    fn set_udp_address(&mut self, addr: SocketAddr) -> (ConnectionInfo<C>, Message<C>) {
        self.udp_address = Some(addr);
        (self.info(), Message::UdpAddressAction(addr))
    }

    fn receive(&mut self) -> Vec<(ConnectionInfo<C>, Message<C>)> {
        let mut buffer: [u8; 255] = [0; 255];
        let mut messages = Vec::new();
        match self.stream.read(&mut buffer) {
            Ok(bytes) => {
                if bytes == 0 {
                    self.close();
                    if self.ident.is_some() {
                        messages.push((self.info(), Message::LobbyLeaveAction));
                    }

                } else {
                    let mut offset = 0;
                    while let Ok(msg) = bincode::deserialize::<Message<C>>(&buffer[offset..]) {

                        offset += bincode::serialized_size::<Message<C>>(&msg).unwrap_or(0) as usize;

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
                if self.ident.is_some() {
                    messages.push((self.info(), Message::LobbyLeaveAction));
                }
            }
        };
        messages
    }

    fn send(&mut self, msg: &Message<C>) {
        let bytes = bincode::serialize(msg).unwrap();
        self.stream.write_all(&bytes[..]).ok();
    }

    fn close(&mut self) {
        info!("[Connection] {} disconnected", self.addr);
        self.open = false;
    }

}

struct Lobby<C: NetworkConfig> {
    id: C::LobbyId,
    open: bool,
    owner: ConnectionInfo<C>,
    connections: Vec<ConnectionInfo<C>>,
    preferences: HashMap<C::PreferenceKey, (C::PreferenceValue, bool)>
}

impl<C: NetworkConfig> Lobby<C> {

    fn owned_by(&self, conn: &ConnectionInfo<C>) -> bool {
        self.owner.token == conn.token
    }

    fn info(&self, conn: &ConnectionInfo<C>) -> Response<C> {
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

    fn start(&mut self) -> Response<C> {
        self.open = false;
        Response::Many(self.connections.iter().map(|conn| {
            Server::connection_event(conn, Message::LobbyStartEvent(self.id.clone()))

        }).collect())
    }

    fn leave(&mut self, conn: &ConnectionInfo<C>) -> Response<C> {
        self.connections.retain(|c| c.token != conn.token);
        Response::Many(vec![
            Server::connection_event(conn, Message::LobbyLeaveEvent(conn.udp_address.expect("Connection without UDP left lobby"))),
            Server::lobby_event(self.id.clone(), Message::LobbyLeaveEvent(conn.udp_address.expect("Connection without UDP left lobby")))
        ])
    }

    fn close(&mut self) -> Response<C> {
        self.open = false;
        let mut r: Vec<Response<C>> = self.connections.iter().map(|c| {
            Server::connection_event(c, Message::LobbyLeaveEvent(c.udp_address.expect("Connection without UDP left lobby")))

        }).collect();
        r.push(Server::public_event(Message::LobbyDestroyEvent(self.id.clone())));
        Response::Many(r)
    }

}

