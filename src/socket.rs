use crate::packet::TCPPacket;
use crate::{tcp, tcpflags};
use anyhow::{Context, Result};
use pnet::packet::{ip::IpNextHeaderProtocols, Packet};
use pnet::transport::{self, TransportChannelType, TransportProtocol, TransportSender};
use pnet::util;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr};
use std::time::SystemTime;

const SOCKET_BUFFER_SIZE: usize = 4380;

// local_addr, remote_addr, local_port, remote_port
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub struct SockID(pub Ipv4Addr, pub Ipv4Addr, pub u16, pub u16);
pub struct Socket {
    pub local_addr: Ipv4Addr,
    pub remote_addr: Ipv4Addr,
    pub local_port: u16,
    pub remote_port: u16,
    pub send_param: SendParam,
    pub recv_param: RecvParam,
    pub status: TcpStatus,
    pub sender: TransportSender,
    pub connected_connection_queue: VecDeque<SockID>,
    pub listening_socket: Option<SockID>,
    pub recv_buffer: Vec<u8>,
    pub retransmission_queue: VecDeque<RetransmissionQueueEntry>,
}

#[derive(Clone, Debug)]
pub struct SendParam {
    pub unacked_seq: u32, // SND.UNA
    pub next: u32,        // SND.NXT
    pub window: u16,      // window size
    pub initial_seq: u32, // 初期受信seq
}

type P = (u32, u32);

#[derive(Clone, Debug)]
pub struct RecvParam {
    pub next: u32,        // RCV.NXT
    pub window: u16,      // window size
    pub initial_seq: u32, // 初期受信seq
    pub tails: BinaryHeap<Reverse<P>>,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TcpStatus {
    Listen,
    SynSent,
    SynRcvd,
    Established,
    FinWait1,
    FinWait2,
    TimeWait,
    CloseWait,
    LastAck,
}

impl Display for TcpStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TcpStatus::Listen => write!(f, "LISTEN"),
            TcpStatus::SynSent => write!(f, "SYNSENT"),
            TcpStatus::SynRcvd => write!(f, "SYNRCVD"),
            TcpStatus::Established => write!(f, "ESTABLISHED"),
            TcpStatus::FinWait1 => write!(f, "FINWAIT1"),
            TcpStatus::FinWait2 => write!(f, "FINWAIT2"),
            TcpStatus::TimeWait => write!(f, "TIMEWAIT"),
            TcpStatus::CloseWait => write!(f, "CLOSEWAIT"),
            TcpStatus::LastAck => write!(f, "LASTACK"),
        }
    }
}

impl Socket {
    pub fn new(
        local_addr: Ipv4Addr,
        remote_addr: Ipv4Addr,
        local_port: u16,
        remote_port: u16,
        status: TcpStatus,
    ) -> Result<Self> {
        let (sender, _) = transport::transport_channel(
            65535,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?;

        Ok(Socket {
            local_addr,
            remote_addr,
            local_port,
            remote_port,
            send_param: SendParam {
                unacked_seq: 0,
                next: 0,
                window: SOCKET_BUFFER_SIZE as u16,
                initial_seq: 0,
            },
            recv_param: RecvParam {
                next: 0,
                window: SOCKET_BUFFER_SIZE as u16,
                initial_seq: 0,
                tails: BinaryHeap::new(),
            },
            status,
            connected_connection_queue: VecDeque::new(),
            listening_socket: None,
            sender,
            recv_buffer: vec![0; SOCKET_BUFFER_SIZE],
            retransmission_queue: VecDeque::new(),
        })
    }

    pub fn send_tcp_packet(
        &mut self,
        seq: u32,
        ack: u32,
        flag: u8,
        payload: &[u8],
    ) -> Result<usize> {
        let mut tcp_packet = TCPPacket::new(payload.len());
        tcp_packet.set_src(self.local_port);
        tcp_packet.set_dest(self.remote_port);
        tcp_packet.set_seq(seq);
        tcp_packet.set_ack(ack);
        // オプションフィールドを使わないので固定
        tcp_packet.set_data_offset(5);
        tcp_packet.set_flag(flag);
        tcp_packet.set_window_size(self.recv_param.window);
        tcp_packet.set_payload(payload);
        tcp_packet.set_checksum(util::ipv4_checksum(
            &tcp_packet.packet(),
            8,
            &[],
            &self.local_addr,
            &self.remote_addr,
            IpNextHeaderProtocols::Tcp,
        ));
        let sent_size = self
            .sender
            .send_to(tcp_packet.clone(), IpAddr::V4(self.remote_addr))
            .context(format!("failed to send: \n{:?}", tcp_packet))?;

        dbg!("sent", &tcp_packet);
        if payload.is_empty() && tcp_packet.get_flag() == tcpflags::ACK {
            return Ok(sent_size);
        }
        self.retransmission_queue
            .push_back(RetransmissionQueueEntry::new(tcp_packet));

        Ok(sent_size)
    }

    pub fn get_sock_id(&self) -> SockID {
        SockID(
            self.local_addr,
            self.remote_addr,
            self.local_port,
            self.remote_port,
        )
    }
}

#[derive(Clone, Debug)]
pub struct RetransmissionQueueEntry {
    pub packet: TCPPacket,
    pub latest_transmission_time: SystemTime,
    pub transmission_count: u8,
}

impl RetransmissionQueueEntry {
    fn new(packet: TCPPacket) -> Self {
        Self {
            packet,
            latest_transmission_time: SystemTime::now(),
            transmission_count: 1,
        }
    }
}
