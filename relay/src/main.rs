use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use common::protocol::RelayPacket;

type RoomMap = Arc<Mutex<HashMap<String, oneshot::Sender<TcpStream>>>>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("0.0.0.0:8081").await?;
    let rooms: RoomMap = Arc::new(Mutex::new(HashMap::new()));

    println!("===================================================");
    println!("   🚀 PUBLIC RENDEZVOUS RELAY SERVER ONLINE 🚀   ");
    println!("   Listening for Host & Client tunnels on port 8081");
    println!("===================================================");

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("[RELAY] Connection attempt from: {}", addr);
        let rooms_clone = rooms.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, rooms_clone).await {
                eprintln!("[RELAY Error]: {}", e);
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream, rooms: RoomMap) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let packet = receive_packet(&mut socket).await?;

    match packet {
        RelayPacket::RegisterHost { room_id } => {
            println!("[RELAY] Host registering Room ID: '{}'", room_id);
            let (bridge_tx, bridge_rx) = oneshot::channel::<TcpStream>();
            
            {
                let mut map = rooms.lock().unwrap();
                map.insert(room_id.clone(), bridge_tx);
            }

            send_packet(&mut socket, &RelayPacket::HostRegistered { room_id: room_id.clone() }).await?;
            println!("[RELAY] Room '{}' registered. Waiting for client...", room_id);

            let mut dummy_buf = [0u8; 1];
            tokio::select! {
                client_res = bridge_rx => {
                    if let Ok(mut client_socket) = client_res {
                        println!("[RELAY] Client joined Room '{}'. Initiating Handshake...", room_id);
                        
                        send_packet(&mut socket, &RelayPacket::ClientConnected).await?;
                        send_packet(&mut client_socket, &RelayPacket::SessionReady).await?;

                        println!("[RELAY] ⚡ FUSING TCP STREAMS FOR ROOM '{}' ⚡", room_id);
                        
                        match tokio::io::copy_bidirectional(&mut socket, &mut client_socket).await {
                            Ok((h2c, c2h)) => {
                                println!("[RELAY] Room '{}' closed. Transferred: Host->Client {} B, Client->Host {} B", room_id, h2c, c2h);
                            }
                            Err(e) => {
                                eprintln!("[RELAY] Room '{}' stream ended: {}", room_id, e);
                            }
                        }
                    }
                }
                _ = socket.read(&mut dummy_buf) => {
                    println!("[RELAY] Host dropped connection. Deleting Room '{}'", room_id);
                    rooms.lock().unwrap().remove(&room_id);
                }
            }
        }
        RelayPacket::ConnectToHost { room_id } => {
            println!("[RELAY] Client requesting connection to Room ID: '{}'", room_id);
            
            let maybe_host_tx = {
                let mut map = rooms.lock().unwrap();
                map.remove(&room_id) 
            };

            match maybe_host_tx {
                Some(host_tx) => {
                    if host_tx.send(socket).is_err() {
                        eprintln!("[RELAY] Failed to bridge socket; Host likely disconnected.");
                    }
                }
                None => {
                    println!("[RELAY] Room ID '{}' not found. Rejecting client.", room_id);
                    send_packet(&mut socket, &RelayPacket::RoomNotFound).await?;
                }
            }
        }
        _ => {
            eprintln!("[RELAY] Invalid handshake packet received.");
        }
    }

    Ok(())
}

async fn send_packet(socket: &mut TcpStream, packet: &RelayPacket) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bytes = bincode::serialize(packet)?;
    let len = bytes.len() as u32;
    socket.write_all(&len.to_be_bytes()).await?;
    socket.write_all(&bytes).await?;
    Ok(())
}

async fn receive_packet(socket: &mut TcpStream) -> Result<RelayPacket, Box<dyn std::error::Error + Send + Sync>> {
    let mut len_buf = [0u8; 4];
    socket.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    socket.read_exact(&mut buf).await?;
    let packet = bincode::deserialize(&buf)?;
    Ok(packet)
}
