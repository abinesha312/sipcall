use anyhow::{anyhow, Context, Result};
use rsip::{
    headers::UntypedHeader,
    message::HeadersExt,
    Method, Request, Response, SipMessage, StatusCode, Uri,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::audio;
use crate::rtp;

const USER_AGENT: &str = "sipcall/0.1.0";

pub async fn listen(bind_addr: &str) -> Result<()> {
    let socket = UdpSocket::bind(bind_addr)
        .await
        .context("Failed to bind UDP socket")?;
    
    let local_addr = socket.local_addr()?;
    println!("SIP listening on {}", local_addr);

    let mut buf = vec![0u8; 65536];
    
    loop {
        let (len, peer_addr) = socket.recv_from(&mut buf).await?;
        let data = &buf[..len];
        
        match String::from_utf8(data.to_vec()) {
            Ok(msg_str) => {
                if let Err(e) = handle_incoming_message(&socket, &msg_str, peer_addr, local_addr).await {
                    eprintln!("Error handling message: {}", e);
                }
            }
            Err(_) => {
                eprintln!("Received non-UTF8 data from {}", peer_addr);
            }
        }
    }
}

async fn handle_incoming_message(
    socket: &UdpSocket,
    msg_str: &str,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
) -> Result<()> {
    let sip_msg: SipMessage = SipMessage::try_from(msg_str.as_bytes())
        .context("Failed to parse SIP message")?;

    match sip_msg {
        SipMessage::Request(req) => {
            handle_request(socket, req, peer_addr, local_addr).await
        }
        SipMessage::Response(resp) => {
            handle_response(resp, peer_addr).await
        }
    }
}

async fn handle_request(
    socket: &UdpSocket,
    request: Request,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
) -> Result<()> {
    match request.method {
        Method::Invite => {
            println!("📞 Incoming call from {}", peer_addr);
            handle_invite(socket, request, peer_addr, local_addr).await
        }
        Method::Ack => {
            println!("✓ Call acknowledged");
            Ok(())
        }
        Method::Bye => {
            println!("📴 Call ended by peer");
            send_bye_response(socket, &request, peer_addr).await?;
            std::process::exit(0);
        }
        Method::Cancel => {
            println!("Call cancelled by peer");
            Ok(())
        }
        _ => {
            println!("Received {} request", request.method);
            Ok(())
        }
    }
}

async fn handle_invite(
    socket: &UdpSocket,
    request: Request,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
) -> Result<()> {
    let call_id = request
        .call_id_header()
        .context("Missing Call-ID")?
        .value()
        .to_string();

    println!("Call-ID: {}", call_id);
    
    let rtp_port = rtp::allocate_port().await?;
    println!("Allocated RTP port: {}", rtp_port);

    send_trying(socket, &request, peer_addr).await?;
    println!("📱 Ringing...");
    
    send_ringing(socket, &request, peer_addr).await?;

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    
    println!("✅ Call answered");
    send_ok(socket, &request, peer_addr, local_addr, rtp_port).await?;

    let audio_active = Arc::new(Mutex::new(true));
    let audio_active_clone = audio_active.clone();
    
    tokio::spawn(async move {
        if let Err(e) = audio::run_audio_session(rtp_port, peer_addr.ip().to_string()).await {
            eprintln!("Audio session error: {}", e);
        }
    });

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\n📴 Hanging up...");
        *audio_active_clone.lock().await = false;
        std::process::exit(0);
    });

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

async fn send_trying(socket: &UdpSocket, request: &Request, peer_addr: SocketAddr) -> Result<()> {
    let response = build_response(request, StatusCode::Trying)?;
    send_response(socket, response, peer_addr).await
}

async fn send_ringing(socket: &UdpSocket, request: &Request, peer_addr: SocketAddr) -> Result<()> {
    let response = build_response(request, StatusCode::Ringing)?;
    send_response(socket, response, peer_addr).await
}

async fn send_ok(
    socket: &UdpSocket,
    request: &Request,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    rtp_port: u16,
) -> Result<()> {
    let mut response = build_response(request, StatusCode::OK)?;
    
    let sdp = format!(
        "v=0\r\n\
         o=sipcall 0 0 IN IP4 {}\r\n\
         s=sipcall\r\n\
         c=IN IP4 {}\r\n\
         t=0 0\r\n\
         m=audio {} RTP/AVP 0\r\n\
         a=rtpmap:0 PCMU/8000\r\n",
        local_addr.ip(),
        local_addr.ip(),
        rtp_port
    );
    
    response.body = sdp.into_bytes();
    response.headers.push(
        rsip::headers::ContentType::new("application/sdp").into()
    );
    response.headers.push(
        rsip::headers::ContentLength::new(response.body.len().to_string()).into()
    );
    
    send_response(socket, response, peer_addr).await
}

async fn send_bye_response(socket: &UdpSocket, request: &Request, peer_addr: SocketAddr) -> Result<()> {
    let response = build_response(request, StatusCode::OK)?;
    send_response(socket, response, peer_addr).await
}

fn build_response(request: &Request, status: StatusCode) -> Result<Response> {
    let via = request.via_header().context("Missing Via")?;
    let from = request.from_header().context("Missing From")?;
    let to = request.to_header().context("Missing To")?;
    let call_id = request.call_id_header().context("Missing Call-ID")?;
    let cseq = request.cseq_header().context("Missing CSeq")?;

    let mut response = Response {
        status_code: status,
        headers: rsip::Headers::default(),
        body: vec![],
        version: request.version.clone(),
    };

    response.headers.push(via.clone().into());
    response.headers.push(from.clone().into());
    response.headers.push(to.clone().into());
    response.headers.push(call_id.clone().into());
    response.headers.push(cseq.clone().into());
    response.headers.push(rsip::headers::UserAgent::new(USER_AGENT).into());

    Ok(response)
}

async fn send_response(socket: &UdpSocket, response: Response, peer_addr: SocketAddr) -> Result<()> {
    let response_str = response.to_string();
    socket
        .send_to(response_str.as_bytes(), peer_addr)
        .await
        .context("Failed to send response")?;
    Ok(())
}

async fn handle_response(response: Response, _peer_addr: SocketAddr) -> Result<()> {
    let status_code: u16 = response.status_code.clone().into();
    match status_code {
        100 => println!("100 Trying"),
        180 => println!("📱 Ringing..."),
        200 => {
            if response.cseq_header().is_ok() {
                println!("✅ Call answered");
            }
        }
        _ => println!("Response: {}", status_code),
    }
    Ok(())
}

pub async fn call(target: &str) -> Result<()> {
    let (dest_addr, sip_uri) = parse_target(target)?;
    
    println!("Calling {} ({})", sip_uri, dest_addr);
    
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("Failed to bind UDP socket")?;
    
    let local_addr = socket.local_addr()?;
    println!("Local SIP port: {}", local_addr.port());
    
    let rtp_port = rtp::allocate_port().await?;
    println!("Local RTP port: {}", rtp_port);

    let call_id = format!("{}@{}", uuid::Uuid::new_v4().to_string(), local_addr.ip());
    
    let from_uri = format!("sip:user@{}:{}", local_addr.ip(), local_addr.port());
    
    let invite = build_invite(&from_uri, &sip_uri, &call_id, local_addr, rtp_port, &dest_addr)?;
    
    socket
        .send_to(invite.to_string().as_bytes(), dest_addr)
        .await
        .context("Failed to send INVITE")?;
    
    println!("📞 Calling...");

    let socket = Arc::new(socket);
    let response_socket = socket.clone();
    let call_id_clone = call_id.clone();
    
    let response_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        let mut ack_sent = false;
        let mut peer_rtp_info: Option<(String, u16)> = None;
        
        loop {
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                response_socket.recv_from(&mut buf)
            ).await {
                Ok(Ok((len, peer_addr))) => {
                    let data = &buf[..len];
                    if let Ok(msg_str) = String::from_utf8(data.to_vec()) {
                        if let Ok(SipMessage::Response(resp)) = SipMessage::try_from(msg_str.as_bytes()) {
                            let status_code: u16 = resp.status_code.clone().into();
                            match status_code {
                                100 => println!("100 Trying"),
                                180 => println!("📱 Ringing..."),
                                200 => {
                                    if !ack_sent {
                                        println!("✅ Call connected");
                                        
                                        if let Some((host, port)) = parse_sdp(&resp.body) {
                                            peer_rtp_info = Some((host, port));
                                        }
                                        
                                        if let Ok(ack) = build_ack(&from_uri, &sip_uri, &call_id_clone, &dest_addr) {
                                            let _ = response_socket.send_to(ack.to_string().as_bytes(), peer_addr).await;
                                            ack_sent = true;
                                        
                                        if let Some((host, _port)) = &peer_rtp_info {
                                                if let Err(e) = audio::run_audio_session(rtp_port, host.clone()).await {
                                                    eprintln!("Audio error: {}", e);
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => println!("Response: {}", resp.status_code),
                            }
                        } else if let Ok(SipMessage::Request(req)) = SipMessage::try_from(msg_str.as_bytes()) {
                            if req.method == Method::Bye {
                                println!("📴 Call ended by peer");
                                std::process::exit(0);
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("Socket error: {}", e);
                    break;
                }
                Err(_) => {
                    eprintln!("Call timeout - no response");
                    break;
                }
            }
        }
    });

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\n📴 Hanging up...");
        std::process::exit(0);
    });

    response_task.await?;
    
    Ok(())
}

fn parse_target(target: &str) -> Result<(SocketAddr, String)> {
    if target.is_empty() {
        return Err(anyhow!("Target cannot be empty"));
    }

    let sip_uri = if target.starts_with("sip:") {
        target.to_string()
    } else {
        format!("sip:{}", target)
    };

    let uri: Uri = Uri::try_from(sip_uri.as_str()).context("Invalid SIP URI")?;
    
    let host = match &uri.host_with_port {
        rsip::HostWithPort { host: h, port: None } => h.to_string(),
        rsip::HostWithPort { host: h, port: Some(p) } => format!("{}:{}", h, p),
    };

    let socket_addr = if host.contains(':') {
        host.parse().context("Invalid host:port")?
    } else {
        format!("{}:5060", host).parse().context("Invalid host")?
    };

    Ok((socket_addr, sip_uri))
}

fn build_invite(
    from_uri: &str,
    to_uri: &str,
    call_id: &str,
    local_addr: SocketAddr,
    rtp_port: u16,
    _dest_addr: &SocketAddr,
) -> Result<Request> {
    let uri: Uri = Uri::try_from(to_uri)?;
    let from_uri_parsed: Uri = Uri::try_from(from_uri)?;
    
    let mut request = Request {
        method: Method::Invite,
        uri: uri.clone(),
        version: rsip::Version::V2,
        headers: rsip::Headers::default(),
        body: vec![],
    };

    let via_branch = format!("z9hG4bK-{}", uuid::Uuid::new_v4().simple());
    request.headers.push(
        rsip::typed::Via {
            version: rsip::Version::V2,
            transport: rsip::Transport::Udp,
            uri: rsip::Uri {
                host_with_port: (rsip::Domain::from(local_addr.ip().to_string()), local_addr.port()).into(),
                ..Default::default()
            },
            params: vec![rsip::Param::Branch(rsip::param::Branch::new(via_branch))],
        }
        .into(),
    );
    
    let from_tag = format!("{}", uuid::Uuid::new_v4().simple());
    request.headers.push(
        rsip::typed::From {
            display_name: None,
            uri: from_uri_parsed.clone(),
            params: vec![rsip::Param::Tag(rsip::param::Tag::new(from_tag))],
        }
        .into(),
    );
    
    request.headers.push(
        rsip::typed::To {
            display_name: None,
            uri: uri.clone(),
            params: vec![],
        }
        .into(),
    );
    
    request.headers.push(rsip::headers::CallId::new(call_id).into());
    
    request.headers.push(
        rsip::typed::CSeq {
            seq: 1,
            method: Method::Invite,
        }
        .into(),
    );
    
    request.headers.push(rsip::headers::UserAgent::new(USER_AGENT).into());
    
    request.headers.push(
        rsip::typed::Contact {
            display_name: None,
            uri: from_uri_parsed,
            params: vec![],
        }
        .into(),
    );
    
    let sdp = format!(
        "v=0\r\n\
         o=sipcall 0 0 IN IP4 {}\r\n\
         s=sipcall\r\n\
         c=IN IP4 {}\r\n\
         t=0 0\r\n\
         m=audio {} RTP/AVP 0\r\n\
         a=rtpmap:0 PCMU/8000\r\n",
        local_addr.ip(),
        local_addr.ip(),
        rtp_port
    );
    
    request.body = sdp.into_bytes();
    request.headers.push(rsip::headers::ContentType::new("application/sdp").into());
    request.headers.push(rsip::headers::ContentLength::new(request.body.len().to_string()).into());

    Ok(request)
}

fn build_ack(from_uri: &str, to_uri: &str, call_id: &str, _dest_addr: &SocketAddr) -> Result<Request> {
    let uri: Uri = Uri::try_from(to_uri)?;
    let from_uri_parsed: Uri = Uri::try_from(from_uri)?;
    
    let mut request = Request {
        method: Method::Ack,
        uri: uri.clone(),
        version: rsip::Version::V2,
        headers: rsip::Headers::default(),
        body: vec![],
    };

    request.headers.push(
        rsip::typed::From {
            display_name: None,
            uri: from_uri_parsed,
            params: vec![],
        }
        .into(),
    );
    
    request.headers.push(
        rsip::typed::To {
            display_name: None,
            uri: uri,
            params: vec![],
        }
        .into(),
    );
    
    request.headers.push(rsip::headers::CallId::new(call_id).into());
    
    request.headers.push(
        rsip::typed::CSeq {
            seq: 1,
            method: Method::Ack,
        }
        .into(),
    );

    Ok(request)
}

fn parse_sdp(body: &[u8]) -> Option<(String, u16)> {
    let sdp = String::from_utf8_lossy(body);
    let mut host = None;
    let mut port = None;
    
    for line in sdp.lines() {
        if line.starts_with("c=") {
            if let Some(ip) = line.split_whitespace().last() {
                host = Some(ip.to_string());
            }
        } else if line.starts_with("m=audio ") {
            if let Some(p) = line.split_whitespace().nth(1) {
                if let Ok(port_num) = p.parse::<u16>() {
                    port = Some(port_num);
                }
            }
        }
    }
    
    match (host, port) {
        (Some(h), Some(p)) => Some((h, p)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_target_sip_uri() {
        let (addr, uri) = parse_target("sip:user@192.168.1.100:5060").unwrap();
        assert_eq!(addr.to_string(), "192.168.1.100:5060");
        assert!(uri.starts_with("sip:"));
    }

    #[test]
    fn test_parse_target_host_port() {
        let (addr, uri) = parse_target("192.168.1.100:5060").unwrap();
        assert_eq!(addr.to_string(), "192.168.1.100:5060");
        assert!(uri.starts_with("sip:"));
    }

    #[test]
    fn test_parse_target_host_only() {
        let (addr, _uri) = parse_target("192.168.1.100").unwrap();
        assert_eq!(addr.to_string(), "192.168.1.100:5060");
    }

    #[test]
    fn test_parse_target_empty() {
        assert!(parse_target("").is_err());
    }

    #[test]
    fn test_build_invite() {
        let result = build_invite(
            "sip:alice@10.0.0.1:5060",
            "sip:bob@10.0.0.2:5060",
            "test-call-id",
            "10.0.0.1:5060".parse().unwrap(),
            10000,
            &"10.0.0.2:5060".parse().unwrap(),
        );
        assert!(result.is_ok());
        let invite = result.unwrap();
        assert_eq!(invite.method, Method::Invite);
    }

    #[test]
    fn test_parse_sdp() {
        let sdp = b"v=0\r\n\
                    o=test 0 0 IN IP4 192.168.1.1\r\n\
                    s=test\r\n\
                    c=IN IP4 192.168.1.1\r\n\
                    t=0 0\r\n\
                    m=audio 8000 RTP/AVP 0\r\n";
        
        let result = parse_sdp(sdp);
        assert!(result.is_some());
        let (host, port) = result.unwrap();
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 8000);
    }
}
