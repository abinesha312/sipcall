use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::net::UdpSocket;

pub async fn allocate_port() -> Result<u16> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("Failed to allocate RTP port")?;
    let addr = socket.local_addr()?;
    Ok(addr.port())
}

pub async fn send_rtp_stream(
    local_port: u16,
    remote_host: String,
    remote_port: u16,
    mut receiver: tokio::sync::mpsc::Receiver<Vec<u8>>,
) -> Result<()> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", local_port))
        .await
        .context("Failed to bind RTP socket")?;
    
    let remote_addr: SocketAddr = format!("{}:{}", remote_host, remote_port)
        .parse()
        .context("Invalid remote RTP address")?;

    println!("RTP sender: {} -> {}", local_port, remote_addr);

    let mut sequence = 0u16;
    let mut timestamp = 0u32;
    let ssrc = rand::random::<u32>();

    while let Some(audio_data) = receiver.recv().await {
        let rtp_packet = build_rtp_packet(sequence, timestamp, ssrc, &audio_data);
        
        if let Err(e) = socket.send_to(&rtp_packet, remote_addr).await {
            eprintln!("RTP send error: {}", e);
        }

        sequence = sequence.wrapping_add(1);
        timestamp = timestamp.wrapping_add(160);
    }

    Ok(())
}

pub async fn receive_rtp_stream(
    local_port: u16,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Result<()> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", local_port))
        .await
        .context("Failed to bind RTP receiver socket")?;

    println!("RTP receiver listening on port {}", local_port);

    let mut buf = vec![0u8; 2048];

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, _peer)) => {
                if len < 12 {
                    continue;
                }

                let payload = buf[12..len].to_vec();
                
                if sender.send(payload).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                eprintln!("RTP receive error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

fn build_rtp_packet(sequence: u16, timestamp: u32, ssrc: u32, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(12 + payload.len());
    
    packet.push(0x80);
    
    packet.push(0);
    
    packet.extend_from_slice(&sequence.to_be_bytes());
    
    packet.extend_from_slice(&timestamp.to_be_bytes());
    
    packet.extend_from_slice(&ssrc.to_be_bytes());
    
    packet.extend_from_slice(payload);
    
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_allocate_port() {
        let port = allocate_port().await.unwrap();
        assert!(port > 0);
    }

    #[test]
    fn test_build_rtp_packet() {
        let payload = vec![1, 2, 3, 4];
        let packet = build_rtp_packet(100, 1000, 12345, &payload);
        
        assert_eq!(packet.len(), 12 + 4);
        assert_eq!(packet[0], 0x80);
        assert_eq!(packet[1], 0);
    }
}
