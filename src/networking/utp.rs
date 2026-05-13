// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::VecDeque,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use tokio::{
    io::{self as tokio_io, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream},
    net::UdpSocket,
    time::{self, Instant},
};

use crate::networking::transport::{PeerConnection, PeerConnectionDirection, PeerEndpoint};

const HEADER_LEN: usize = 20;
const UTP_VERSION: u8 = 1;
const TYPE_DATA: u8 = 0;
const TYPE_FIN: u8 = 1;
const TYPE_STATE: u8 = 2;
const TYPE_RESET: u8 = 3;
const TYPE_SYN: u8 = 4;

const RECEIVE_WINDOW: u32 = 1_048_576;
const MAX_PAYLOAD: usize = 1_200;
const STREAM_BUFFER: usize = 256 * 1024;
const MAX_INFLIGHT_BYTES: usize = 64 * 1024;
const MAX_INFLIGHT_PACKETS: usize = 64;
const CONNECT_RETRIES: usize = 4;
const CONNECT_RETRY_TIMEOUT: Duration = Duration::from_millis(400);
const RETRANSMIT_AFTER: Duration = Duration::from_millis(500);
const MAX_RETRANSMITS: u8 = 8;

/// Minimal homegrown BEP 29/uTP outbound transport.
///
/// This currently supports stream-like outbound connections and basic reliability.
/// Congestion control and inbound UDP demultiplexing are intentionally left for a
/// later transport-runtime pass.
pub struct UtpPeerTransport;

impl UtpPeerTransport {
    pub async fn connect(remote_addr: SocketAddr) -> io::Result<PeerConnection> {
        let socket = bind_outbound_socket(remote_addr).await?;
        socket.connect(remote_addr).await?;

        let start = Instant::now();
        let receive_connection_id = random_connection_id();
        let send_connection_id = receive_connection_id.wrapping_add(1);
        let initial_seq_nr = rand::random::<u16>();

        let syn = UtpPacket {
            packet_type: TYPE_SYN,
            connection_id: receive_connection_id,
            timestamp_microseconds: timestamp_microseconds(start),
            timestamp_difference_microseconds: 0,
            wnd_size: RECEIVE_WINDOW,
            seq_nr: initial_seq_nr,
            ack_nr: 0,
            payload: Vec::new(),
        };
        let syn_bytes = syn.encode();
        let mut buf = vec![0_u8; 2_048];

        let state = loop {
            socket.send(&syn_bytes).await?;

            match time::timeout(CONNECT_RETRY_TIMEOUT, socket.recv(&mut buf)).await {
                Ok(Ok(n)) => {
                    let packet = UtpPacket::decode(&buf[..n])?;
                    if packet.packet_type == TYPE_RESET {
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionReset,
                            "uTP peer reset during connect",
                        ));
                    }
                    if packet.packet_type == TYPE_STATE
                        && packet.connection_id == receive_connection_id
                        && packet.ack_nr == initial_seq_nr
                    {
                        break UtpDriverState {
                            send_connection_id,
                            receive_connection_id,
                            next_send_seq_nr: initial_seq_nr.wrapping_add(1),
                            last_remote_seq_nr: packet.seq_nr,
                            last_remote_timestamp_microseconds: packet.timestamp_microseconds,
                            start,
                        };
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => {}
            }

            if elapsed_connect_attempts(start) >= CONNECT_RETRIES {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "uTP connect timed out",
                ));
            }
        };

        let (client_stream, driver_stream) = tokio_io::duplex(STREAM_BUFFER);
        tokio::spawn(async move {
            if let Err(error) = run_utp_driver(socket, driver_stream, state).await {
                tracing::debug!(%remote_addr, %error, "uTP driver stopped");
            }
        });

        Ok(PeerConnection::new(
            client_stream,
            PeerEndpoint::utp(remote_addr),
            remote_addr,
            PeerConnectionDirection::Outgoing,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UtpPacket {
    packet_type: u8,
    connection_id: u16,
    timestamp_microseconds: u32,
    timestamp_difference_microseconds: u32,
    wnd_size: u32,
    seq_nr: u16,
    ack_nr: u16,
    payload: Vec<u8>,
}

impl UtpPacket {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + self.payload.len());
        bytes.push((self.packet_type << 4) | UTP_VERSION);
        bytes.push(0);
        bytes.extend_from_slice(&self.connection_id.to_be_bytes());
        bytes.extend_from_slice(&self.timestamp_microseconds.to_be_bytes());
        bytes.extend_from_slice(&self.timestamp_difference_microseconds.to_be_bytes());
        bytes.extend_from_slice(&self.wnd_size.to_be_bytes());
        bytes.extend_from_slice(&self.seq_nr.to_be_bytes());
        bytes.extend_from_slice(&self.ack_nr.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < HEADER_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "uTP packet shorter than header",
            ));
        }

        let packet_type = bytes[0] >> 4;
        let version = bytes[0] & 0x0f;
        if version != UTP_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported uTP packet version",
            ));
        }
        if packet_type > TYPE_SYN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported uTP packet type",
            ));
        }
        let payload_offset = parse_extension_chain(bytes, bytes[1])?;

        Ok(Self {
            packet_type,
            connection_id: u16::from_be_bytes([bytes[2], bytes[3]]),
            timestamp_microseconds: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            timestamp_difference_microseconds: u32::from_be_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11],
            ]),
            wnd_size: u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            seq_nr: u16::from_be_bytes([bytes[16], bytes[17]]),
            ack_nr: u16::from_be_bytes([bytes[18], bytes[19]]),
            payload: bytes[payload_offset..].to_vec(),
        })
    }
}

fn parse_extension_chain(bytes: &[u8], first_extension: u8) -> io::Result<usize> {
    let mut extension = first_extension;
    let mut offset = HEADER_LEN;

    while extension != 0 {
        if bytes.len() < offset + 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "uTP extension header truncated",
            ));
        }

        let next_extension = bytes[offset];
        let extension_len = bytes[offset + 1] as usize;
        offset += 2;

        if bytes.len() < offset + extension_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "uTP extension body truncated",
            ));
        }

        offset += extension_len;
        extension = next_extension;
    }

    Ok(offset)
}

struct UtpDriverState {
    send_connection_id: u16,
    receive_connection_id: u16,
    next_send_seq_nr: u16,
    last_remote_seq_nr: u16,
    last_remote_timestamp_microseconds: u32,
    start: Instant,
}

#[derive(Clone)]
struct SentPacket {
    packet_type: u8,
    seq_nr: u16,
    payload: Vec<u8>,
    sent_at: Instant,
    retransmits: u8,
}

async fn run_utp_driver(
    socket: UdpSocket,
    local_stream: DuplexStream,
    mut state: UtpDriverState,
) -> io::Result<()> {
    let (mut local_reader, mut local_writer) = tokio_io::split(local_stream);
    let mut udp_buf = vec![0_u8; 2_048];
    let mut local_buf = vec![0_u8; MAX_PAYLOAD];
    let mut pending_payloads: VecDeque<Vec<u8>> = VecDeque::new();
    let mut unacked_packets: VecDeque<SentPacket> = VecDeque::new();
    let mut local_eof = false;
    let mut retransmit_tick = time::interval(Duration::from_millis(100));

    loop {
        flush_pending_payloads(
            &socket,
            &mut state,
            &mut pending_payloads,
            &mut unacked_packets,
        )
        .await?;

        if local_eof && pending_payloads.is_empty() && unacked_packets.is_empty() {
            send_control_packet(&socket, &mut state, TYPE_FIN).await?;
            return Ok(());
        }

        tokio::select! {
            read_result = local_reader.read(&mut local_buf), if !local_eof && pending_payloads.len() < MAX_INFLIGHT_PACKETS => {
                let bytes_read = read_result?;
                if bytes_read == 0 {
                    local_eof = true;
                } else {
                    pending_payloads.push_back(local_buf[..bytes_read].to_vec());
                }
            }
            recv_result = socket.recv(&mut udp_buf) => {
                let bytes_read = recv_result?;
                let packet = UtpPacket::decode(&udp_buf[..bytes_read])?;
                process_incoming_packet(&socket, &mut local_writer, &mut state, &mut unacked_packets, packet).await?;
            }
            _ = retransmit_tick.tick() => {
                retransmit_due_packets(&socket, &state, &mut unacked_packets).await?;
            }
        }
    }
}

async fn flush_pending_payloads(
    socket: &UdpSocket,
    state: &mut UtpDriverState,
    pending_payloads: &mut VecDeque<Vec<u8>>,
    unacked_packets: &mut VecDeque<SentPacket>,
) -> io::Result<()> {
    while let Some(payload) = pending_payloads.front() {
        if unacked_payload_bytes(unacked_packets) + payload.len() > MAX_INFLIGHT_BYTES
            || unacked_packets.len() >= MAX_INFLIGHT_PACKETS
        {
            break;
        }

        let payload = pending_payloads.pop_front().expect("front checked above");
        let seq_nr = state.next_send_seq_nr;
        state.next_send_seq_nr = state.next_send_seq_nr.wrapping_add(1);
        let packet = data_packet(state, seq_nr, payload.clone());
        socket.send(&packet.encode()).await?;
        unacked_packets.push_back(SentPacket {
            packet_type: TYPE_DATA,
            seq_nr,
            payload,
            sent_at: Instant::now(),
            retransmits: 0,
        });
    }

    Ok(())
}

async fn process_incoming_packet<W>(
    socket: &UdpSocket,
    local_writer: &mut W,
    state: &mut UtpDriverState,
    unacked_packets: &mut VecDeque<SentPacket>,
    packet: UtpPacket,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    if packet.connection_id != state.receive_connection_id {
        return Ok(());
    }

    match packet.packet_type {
        TYPE_STATE => {
            acknowledge_packets(unacked_packets, packet.ack_nr);
            state.last_remote_timestamp_microseconds = packet.timestamp_microseconds;
        }
        TYPE_DATA => {
            state.last_remote_timestamp_microseconds = packet.timestamp_microseconds;
            let expected_seq_nr = state.last_remote_seq_nr.wrapping_add(1);
            if packet.seq_nr == expected_seq_nr {
                if !packet.payload.is_empty() {
                    local_writer.write_all(&packet.payload).await?;
                }
                state.last_remote_seq_nr = packet.seq_nr;
            }
            send_state_packet(socket, state).await?;
        }
        TYPE_FIN => {
            state.last_remote_timestamp_microseconds = packet.timestamp_microseconds;
            let expected_seq_nr = state.last_remote_seq_nr.wrapping_add(1);
            if packet.seq_nr == expected_seq_nr {
                state.last_remote_seq_nr = packet.seq_nr;
            }
            send_state_packet(socket, state).await?;
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "uTP peer closed stream",
            ));
        }
        TYPE_RESET => {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "uTP peer reset stream",
            ));
        }
        TYPE_SYN => {}
        _ => {}
    }

    Ok(())
}

async fn retransmit_due_packets(
    socket: &UdpSocket,
    state: &UtpDriverState,
    unacked_packets: &mut VecDeque<SentPacket>,
) -> io::Result<()> {
    let now = Instant::now();
    for sent in unacked_packets.iter_mut() {
        if now.duration_since(sent.sent_at) < RETRANSMIT_AFTER {
            continue;
        }
        if sent.retransmits >= MAX_RETRANSMITS {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "uTP retransmit budget exhausted",
            ));
        }

        sent.sent_at = now;
        sent.retransmits = sent.retransmits.saturating_add(1);
        let packet = UtpPacket {
            packet_type: sent.packet_type,
            connection_id: state.send_connection_id,
            timestamp_microseconds: timestamp_microseconds(state.start),
            timestamp_difference_microseconds: timestamp_difference_microseconds(state),
            wnd_size: RECEIVE_WINDOW,
            seq_nr: sent.seq_nr,
            ack_nr: state.last_remote_seq_nr,
            payload: sent.payload.clone(),
        };
        socket.send(&packet.encode()).await?;
    }

    Ok(())
}

async fn send_control_packet(
    socket: &UdpSocket,
    state: &mut UtpDriverState,
    packet_type: u8,
) -> io::Result<()> {
    let seq_nr = state.next_send_seq_nr;
    state.next_send_seq_nr = state.next_send_seq_nr.wrapping_add(1);
    let packet = UtpPacket {
        packet_type,
        connection_id: state.send_connection_id,
        timestamp_microseconds: timestamp_microseconds(state.start),
        timestamp_difference_microseconds: timestamp_difference_microseconds(state),
        wnd_size: RECEIVE_WINDOW,
        seq_nr,
        ack_nr: state.last_remote_seq_nr,
        payload: Vec::new(),
    };
    socket.send(&packet.encode()).await?;
    Ok(())
}

async fn send_state_packet(socket: &UdpSocket, state: &UtpDriverState) -> io::Result<()> {
    let packet = UtpPacket {
        packet_type: TYPE_STATE,
        connection_id: state.send_connection_id,
        timestamp_microseconds: timestamp_microseconds(state.start),
        timestamp_difference_microseconds: timestamp_difference_microseconds(state),
        wnd_size: RECEIVE_WINDOW,
        seq_nr: state.next_send_seq_nr.wrapping_sub(1),
        ack_nr: state.last_remote_seq_nr,
        payload: Vec::new(),
    };
    socket.send(&packet.encode()).await?;
    Ok(())
}

fn data_packet(state: &UtpDriverState, seq_nr: u16, payload: Vec<u8>) -> UtpPacket {
    UtpPacket {
        packet_type: TYPE_DATA,
        connection_id: state.send_connection_id,
        timestamp_microseconds: timestamp_microseconds(state.start),
        timestamp_difference_microseconds: timestamp_difference_microseconds(state),
        wnd_size: RECEIVE_WINDOW,
        seq_nr,
        ack_nr: state.last_remote_seq_nr,
        payload,
    }
}

fn acknowledge_packets(unacked_packets: &mut VecDeque<SentPacket>, ack_nr: u16) {
    while unacked_packets
        .front()
        .is_some_and(|sent| seq_lte(sent.seq_nr, ack_nr))
    {
        unacked_packets.pop_front();
    }
}

fn seq_lte(lhs: u16, rhs: u16) -> bool {
    lhs == rhs || rhs.wrapping_sub(lhs) < 0x8000
}

fn unacked_payload_bytes(unacked_packets: &VecDeque<SentPacket>) -> usize {
    unacked_packets
        .iter()
        .map(|packet| packet.payload.len())
        .sum()
}

fn timestamp_microseconds(start: Instant) -> u32 {
    start.elapsed().as_micros() as u32
}

fn timestamp_difference_microseconds(state: &UtpDriverState) -> u32 {
    timestamp_microseconds(state.start).wrapping_sub(state.last_remote_timestamp_microseconds)
}

fn random_connection_id() -> u16 {
    loop {
        let id = rand::random::<u16>();
        if id != u16::MAX {
            return id;
        }
    }
}

fn elapsed_connect_attempts(start: Instant) -> usize {
    let elapsed = start.elapsed().as_millis();
    let retry_ms = CONNECT_RETRY_TIMEOUT.as_millis().max(1);
    (elapsed / retry_ms) as usize
}

async fn bind_outbound_socket(remote_addr: SocketAddr) -> io::Result<UdpSocket> {
    let bind_addr = match remote_addr.ip() {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    UdpSocket::bind(bind_addr).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn packet_round_trips() {
        let packet = UtpPacket {
            packet_type: TYPE_DATA,
            connection_id: 123,
            timestamp_microseconds: 456,
            timestamp_difference_microseconds: 789,
            wnd_size: 1_024,
            seq_nr: 65_530,
            ack_nr: 42,
            payload: b"hello".to_vec(),
        };

        let decoded = UtpPacket::decode(&packet.encode()).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn decode_skips_extension_chain() {
        let packet = UtpPacket {
            packet_type: TYPE_DATA,
            connection_id: 123,
            timestamp_microseconds: 456,
            timestamp_difference_microseconds: 789,
            wnd_size: 1_024,
            seq_nr: 65_530,
            ack_nr: 42,
            payload: b"hello".to_vec(),
        };
        let mut bytes = packet.encode();
        bytes[1] = 1;
        bytes.splice(HEADER_LEN..HEADER_LEN, [0, 3, 1, 2, 3]);

        let decoded = UtpPacket::decode(&bytes).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn sequence_comparison_wraps() {
        assert!(seq_lte(65_535, 0));
        assert!(seq_lte(0, 1));
        assert!(!seq_lte(10, 9));
    }

    #[tokio::test]
    async fn outbound_connection_exchanges_payload_with_utp_peer() {
        let server = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .unwrap();
        let server_addr = server.local_addr().unwrap();

        let server_task = tokio::spawn(async move {
            let mut buf = vec![0_u8; 2_048];
            let (n, client_addr) = server.recv_from(&mut buf).await.unwrap();
            let syn = UtpPacket::decode(&buf[..n]).unwrap();
            assert_eq!(syn.packet_type, TYPE_SYN);

            let server_seq_nr = 77;
            let state = UtpPacket {
                packet_type: TYPE_STATE,
                connection_id: syn.connection_id,
                timestamp_microseconds: 1,
                timestamp_difference_microseconds: 0,
                wnd_size: RECEIVE_WINDOW,
                seq_nr: server_seq_nr,
                ack_nr: syn.seq_nr,
                payload: Vec::new(),
            };
            server.send_to(&state.encode(), client_addr).await.unwrap();

            let (n, client_addr) = server.recv_from(&mut buf).await.unwrap();
            let data = UtpPacket::decode(&buf[..n]).unwrap();
            assert_eq!(data.packet_type, TYPE_DATA);
            assert_eq!(data.connection_id, syn.connection_id.wrapping_add(1));
            assert_eq!(data.payload, b"ping");

            let ack = UtpPacket {
                packet_type: TYPE_STATE,
                connection_id: syn.connection_id,
                timestamp_microseconds: 2,
                timestamp_difference_microseconds: 0,
                wnd_size: RECEIVE_WINDOW,
                seq_nr: server_seq_nr,
                ack_nr: data.seq_nr,
                payload: Vec::new(),
            };
            server.send_to(&ack.encode(), client_addr).await.unwrap();

            let echo = UtpPacket {
                packet_type: TYPE_DATA,
                connection_id: syn.connection_id,
                timestamp_microseconds: 3,
                timestamp_difference_microseconds: 0,
                wnd_size: RECEIVE_WINDOW,
                seq_nr: server_seq_nr + 1,
                ack_nr: data.seq_nr,
                payload: data.payload,
            };
            server.send_to(&echo.encode(), client_addr).await.unwrap();
        });

        let mut connection = UtpPeerTransport::connect(server_addr).await.unwrap();
        connection.stream.write_all(b"ping").await.unwrap();

        let mut echoed = [0_u8; 4];
        connection.stream.read_exact(&mut echoed).await.unwrap();

        assert_eq!(&echoed, b"ping");
        server_task.await.unwrap();
    }
}
