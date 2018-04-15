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
use std::io::{Error as IOError, ErrorKind, Read, Write};
use std::time::{Duration, Instant};
use std::net::{UdpSocket, TcpListener, TcpStream, ToSocketAddrs};


// External Dependencies ------------------------------------------------------
use bincode;


// Internal Dependencies ------------------------------------------------------
use ::{ClientAction, Error, NetworkConfig, Message, UdpToken, TcpAddress, UdpAddress, ServerEvent};


// Server Implementation ------------------------------------------------------
pub struct Server<C: NetworkConfig> {
    tick_rate: u64,
    lobbies: HashMap<C::LobbyId, Lobby<C>>,
    connections: HashMap<TcpAddress, Connection<C>>,
    tokens: HashMap<UdpToken, TcpAddress>,
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

    pub fn listen<A: ToSocketAddrs + Clone>(mut self, address: A) -> Result<(), IOError> {

        let listener = TcpListener::bind(address.clone())?;
        listener.set_nonblocking(true)?;

        let udp_listener = UdpSocket::bind(address)?;
        udp_listener.set_nonblocking(true)?;

        info!("[Server] Started");

        loop {

            // TCP Control Messages
            while let Ok((stream, addr)) = listener.accept() {
                self.connect(stream, TcpAddress::new(addr)).ok();
            }

            // UDP Idenfification Messages
            for (conn, action) in self.receive(&udp_listener) {
                let response = match action {
                    ClientAction::Identify(_) => {
                        Server::connection_error(&conn, Error::AlreadyIdentified)
                    },
                    ClientAction::Ready => {
                        info!("[Connection] {} identified", conn);
                        let mut r = vec![Server::connection_event(&conn, ServerEvent::Identify(conn.token))];
                        for lobby in self.lobbies.values() {
                            r.push(lobby.info(&conn));
                        }
                        Response::Many(r)
                    },
                    ClientAction::UdpAddress(addr) => {
                        info!("[Connection] {} assigned UDP address {}", conn, addr);
                        Server::connection_event(&conn, ServerEvent::UdpAddress(addr))
                    },
                    ClientAction::LobbyCreate(id) => {
                        if let Some(id) = conn.lobby_id.clone() {
                            Server::connection_error(&conn, Error::AlreadyInLobby(id))

                        } else if !conn.udp_address_known() {
                            Server::error_address(&conn)

                        } else if self.lobbies.contains_key(&id) {
                            Server::connection_error(&conn, Error::LobbyAlreadyExists(id))

                        } else {
                            self.set_lobby(&conn, Some(id.clone()));
                            self.create_lobby(&conn, id)
                        }
                    },
                    ClientAction::LobbyJoin(id, payload) => {
                        if let Some(id) = conn.lobby_id.clone() {
                            Server::connection_error(&conn, Error::AlreadyInLobby(id))

                        } else if !conn.udp_address_known() {
                            Server::error_address(&conn)

                        } else if let Some(lobby) = self.lobbies.get_mut(&id) {
                            Server::connection_event(&lobby.owner, ServerEvent::LobbyJoinRequest {
                                id: lobby.id.clone(),
                                ident: conn.ident.clone(),
                                addr: conn.udp,
                                payload
                            })

                        } else {
                            Server::connection_error(&conn, Error::LobbyNotFound(id))
                        }
                    },
                    ClientAction::LobbyJoinResponse(addr, allow) => {
                        if let Some(id) = conn.lobby_id.clone() {
                            let join_conn = self.connections.values().find(|c| c.udp == addr).map(|c| c.info());
                            if let Some(conn) = join_conn {
                                if allow {
                                    info!("[Lobby] \"{}\" allowed join for {}", id, conn);
                                    self.set_lobby(&conn, Some(id.clone()));
                                    self.join_lobby(&conn, id)

                                } else {
                                    info!("[Lobby] \"{}\" denied join for {}", id, conn);
                                    Server::connection_error(&conn, Error::LobbyJoinDenied(id))
                                }

                            } else {
                                Server::connection_error(&conn, Error::LobbyNotFound(id))
                            }

                        } else {
                            Server::connection_error(&conn, Error::NotInAnyLobby)
                        }
                    },
                    ClientAction::LobbyPreference { key, value, is_public } => match self.get_lobby(&conn) {
                        Ok(lobby) => if lobby.owned_by(&conn) {
                            info!("[Lobby] \"{}\" preference \"{}\" set to \"{}\" by {}", lobby.id, key, value, conn);
                            lobby.preferences.insert(key.clone(), (value.clone(), is_public));
                            if is_public {
                                Server::public_event(ServerEvent::LobbyPreference {
                                    id: lobby.id.clone(),
                                    key,
                                    value
                                })

                            } else {
                                Server::lobby_event(lobby.id.clone(), ServerEvent::LobbyPreference {
                                    id: lobby.id.clone(),
                                    key,
                                    value
                                })
                            }

                        } else {
                            Server::connection_event(&lobby.owner, ServerEvent::LobbyPreferenceRequest {
                                id: lobby.id.clone(),
                                ident: conn.ident.clone(),
                                addr: conn.udp,
                                key,
                                value
                            })
                        },
                        Err(event) => event
                    },
                    ClientAction::LobbyLeave => {
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
                    ClientAction::LobbyStart => match self.get_lobby_if_owner(&conn) {
                        Ok(lobby) => {
                            info!("[Lobby] \"{}\" started by {}", lobby.id, conn);
                            lobby.start()
                        }
                        Err(event) => event
                    }
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

    fn connect(&mut self, stream: TcpStream, addr: TcpAddress) -> Result<(), IOError> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let conn = Connection::new(addr, stream);
        self.tokens.insert(conn.token, conn.addr);
        self.connections.insert(addr, conn);
        info!("[Connection] {} connected", addr);
        Ok(())
    }

    fn receive(&mut self, udp_listener: &UdpSocket) -> Vec<(ConnectionInfo<C>, ClientAction<C>)> {

        let mut actions = Vec::new();
        for conn in &mut self.connections.values_mut() {
            actions.append(&mut conn.receive());
        }

        let mut buffer: [u8; 8] = [0; 8];
        while let Ok((len, udp)) = udp_listener.recv_from(&mut buffer) {
            if let Some(token) = UdpToken::try_from_buffer(&buffer, len) {
                if let Some(tcp) = self.tokens.remove(&token) {
                    if let Some(conn) = self.connections.get_mut(&tcp) {
                        actions.push(conn.set_udp_address(UdpAddress::new(udp)));
                    }
                }
            }
        }

        self.connections.retain(|_, conn| conn.open);
        actions

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
                            if let Some(conn) = self.connections.get_mut(&conn.tcp) {
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
        if let Some(conn) = self.connections.get_mut(&conn.tcp) {
            conn.lobby_id = id;
        }
    }

    fn join_lobby(&mut self, conn: &ConnectionInfo<C>, id: C::LobbyId) -> Response<C> {
        if let Some(lobby) = self.lobbies.get_mut(&id) {

            let mut r: Vec<Response<C>> = vec![
                Server::lobby_event(id.clone(), ServerEvent::LobbyJoin {
                    id: id.clone(),
                    addr: conn.udp,
                    ident: conn.ident.clone(),
                    is_owner: false
                })
            ];

            for c in &lobby.connections {
                r.push(Server::connection_event(conn, ServerEvent::LobbyJoin {
                    id: id.clone(),
                    addr: c.udp,
                    ident: c.ident.clone(),
                    is_owner: lobby.owned_by(c)
                }));
            }

            for (key, &(ref value, _)) in &lobby.preferences {
                r.push(Server::connection_event(conn, ServerEvent::LobbyPreference {
                    id: lobby.id.clone(),
                    key: key.clone(),
                    value: value.clone()
                }));
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
            Server::public_event(ServerEvent::LobbyCreate(id.clone())),
            Server::connection_event(conn, ServerEvent::LobbyJoin {
                id,
                addr: conn.udp,
                ident: conn.ident.clone(),
                is_owner: true
            })
        ])

    }

    fn get_lobby(&mut self, conn: &ConnectionInfo<C>) -> Result<&mut Lobby<C>, Response<C>> {
        if !conn.udp_address_known() {
            Err(Server::error_address(conn))

        } else if let Some(ref id) = conn.lobby_id {
            if let Some(lobby) = self.lobbies.get_mut(&id) {
                Ok(lobby)

            // Lobby not found
            } else {
                Err(Server::connection_error(conn, Error::LobbyNotFound(id.clone())))
            }

        // Not in any Lobby
        } else {
            Err(Server::connection_error(conn, Error::NotInAnyLobby))
        }
    }

    fn get_lobby_if_owner(&mut self, conn: &ConnectionInfo<C>) -> Result<&mut Lobby<C>, Response<C>> {
        if !conn.udp_address_known() {
            Err(Server::error_address(conn))

        } else if let Some(ref id) = conn.lobby_id {
            if let Some(lobby) = self.lobbies.get_mut(&id) {
                if lobby.owned_by(&conn) {
                    Ok(lobby)

                // Not owner
                } else {
                    Err(Server::connection_error(conn, Error::LobbyNotFound(id.clone())))
                }

            // Lobby not found
            } else {
                Err(Server::connection_error(conn, Error::AlreadyInLobby(id.clone())))
            }

        // Not in any Lobby
        } else {
            Err(Server::connection_error(conn, Error::NotInAnyLobby))
        }
    }

    fn connection_event(conn: &ConnectionInfo<C>, event: ServerEvent<C>) -> Response<C> {
        Response::Single(Recipient::Connection(conn.tcp), event)
    }

    fn lobby_event(id: C::LobbyId, event: ServerEvent<C>) -> Response<C> {
        Response::Single(Recipient::Lobby(id), event)
    }

    fn public_event(event: ServerEvent<C>) -> Response<C> {
        Response::Single(Recipient::Everyone, event)
    }

    fn error_address(conn: &ConnectionInfo<C>) -> Response<C> {
        Response::Single(Recipient::Connection(conn.tcp), ServerEvent::Error(Error::NoUdpAddress))
    }

    fn connection_error(conn: &ConnectionInfo<C>, err: Error<C::LobbyId>) -> Response<C> {
        Response::Single(Recipient::Connection(conn.tcp), ServerEvent::Error(err))
    }

}

enum Response<C: NetworkConfig> {
    Empty,
    Single(Recipient<C>, ServerEvent<C>),
    Many(Vec<Response<C>>)
}

enum Recipient<C: NetworkConfig> {
    Everyone,
    Connection(TcpAddress),
    Lobby(C::LobbyId)
}

#[derive(Clone)]
struct ConnectionInfo<C: NetworkConfig> {
    tcp: TcpAddress,
    udp: UdpAddress,
    lobby_id: Option<C::LobbyId>,
    ident: C::ConnectionIdentifier,
    token: UdpToken
}

impl<C: NetworkConfig> ConnectionInfo<C> {

    fn udp_address_known(&self) -> bool {
        self.udp.is_valid()
    }
}

impl<C: NetworkConfig> fmt::Display for ConnectionInfo<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<\"{}\"@{}@{:?}>", self.ident, self.tcp, self.udp)
    }
}

struct Connection<C: NetworkConfig> {
    lobby_id: Option<C::LobbyId>,
    open: bool,
    addr: TcpAddress,
    udp: UdpAddress,
    stream: TcpStream,
    token: UdpToken,
    ident: Option<C::ConnectionIdentifier>
}

impl<C: NetworkConfig> Connection<C> {

    fn new(addr: TcpAddress, stream: TcpStream) -> Self {
        Self {
            lobby_id: None,
            open: true,
            addr,
            udp: UdpAddress::empty(),
            stream,
            token: UdpToken::new(),
            ident: None
        }
    }

    fn info(&self) -> ConnectionInfo<C> {
        ConnectionInfo {
            tcp: self.addr,
            lobby_id: self.lobby_id.clone(),
            udp: self.udp,
            token: self.token,
            ident: self.ident.clone().expect("Message forwarded before identification")
        }
    }

    fn set_udp_address(&mut self, addr: UdpAddress) -> (ConnectionInfo<C>, ClientAction<C>) {
        self.udp = addr;
        (self.info(), ClientAction::UdpAddress(addr))
    }

    fn receive(&mut self) -> Vec<(ConnectionInfo<C>, ClientAction<C>)> {
        let mut buffer: [u8; 255] = [0; 255];
        let mut actions = Vec::new();
        match self.stream.read(&mut buffer) {
            Ok(bytes) => {
                if bytes == 0 {
                    self.close();
                    if self.ident.is_some() {
                        actions.push((self.info(), ClientAction::LobbyLeave));
                    }

                } else {
                    let mut offset = 0;
                    while let Ok(msg) = bincode::deserialize::<Message<C>>(&buffer[offset..]) {

                        offset += bincode::serialized_size::<Message<C>>(&msg).unwrap_or(0) as usize;

                        if self.ident.is_some() {
                            if let Message::ClientAction(action) = msg {
                                actions.push((self.info(), action));
                            }

                        // Wait for identification
                        } else if let Message::ClientAction(ClientAction::Identify(ident)) = msg {
                            self.ident = Some(ident);
                            actions.push((self.info(), ClientAction::Ready));

                        } else {
                            // TODO response with Error::NoIdentification
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
                    actions.push((self.info(), ClientAction::LobbyLeave));
                }
            }
        };
        actions
    }

    fn send(&mut self, event: &ServerEvent<C>) {
        let msg = Message::ServerEvent(event.clone());
        let bytes = bincode::serialize(&msg).unwrap();
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
            Server::connection_event(conn, ServerEvent::LobbyCreate(self.id.clone()))
        ];
        for (key, &(ref value, public)) in &self.preferences {
            if public {
                r.push(Server::connection_event(conn, ServerEvent::LobbyPreference {
                    id: self.id.clone(),
                    key: key.clone(),
                    value: value.clone()
                }));
            }
        }
        Response::Many(r)
    }

    fn start(&mut self) -> Response<C> {
        self.open = false;
        Response::Many(self.connections.iter().map(|conn| {
            Server::connection_event(conn, ServerEvent::LobbyStart(self.id.clone()))

        }).collect())
    }

    fn leave(&mut self, conn: &ConnectionInfo<C>) -> Response<C> {
        self.connections.retain(|c| c.token != conn.token);
        Response::Many(vec![
            Server::connection_event(conn, ServerEvent::LobbyLeave(conn.udp)),
            Server::lobby_event(self.id.clone(), ServerEvent::LobbyLeave(conn.udp))
        ])
    }

    fn close(&mut self) -> Response<C> {
        self.open = false;
        let mut r: Vec<Response<C>> = self.connections.iter().map(|c| {
            Server::connection_event(c, ServerEvent::LobbyLeave(c.udp))

        }).collect();
        r.push(Server::public_event(ServerEvent::LobbyDestroy(self.id.clone())));
        Response::Many(r)
    }

}

