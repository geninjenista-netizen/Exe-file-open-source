use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use common::protocol::{RelayPacket, Command};

type RoomMap = Arc<Mutex<HashMap<String, oneshot::Sender<TcpStream>>>>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    let listener = TcpListener::bind("0.0.0.0:8081").await?;
    let rooms: RoomMap = Arc::new(Mutex::new(HashMap::new()));

    println!("===================================================");
    println!("   🚀 SMART DYNAMIC RELAY SERVER ONLINE 🚀   ");
    println!("   Listening on port 8081");
    println!("===================================================");

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("[RELAY] New connection from: {}", addr);
        let rooms_clone = rooms.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, rooms_clone).await {
                eprintln!("[RELAY Error] {}", e);
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream, rooms: RoomMap) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let packet = receive_packet(&mut socket).await?;

    match packet {
        RelayPacket::RegisterHost { room_id } => {
            handle_host_registration(socket, room_id, rooms).await?;
        }
        RelayPacket::ConnectToHost { room_id } => {
            handle_client_connection(socket, room_id, rooms).await?;
        }
        _ => {
            eprintln!("[RELAY] Unexpected initial packet");
        }
    }
    Ok(())
}

async fn handle_host_registration(mut host_socket: TcpStream, room_id: String, rooms: RoomMap) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("[RELAY] Host registered room: {}", room_id);

    let (bridge_tx, bridge_rx) = oneshot::channel::<TcpStream>();

    {
        let mut map = rooms.lock().unwrap();
        map.insert(room_id.clone(), bridge_tx);
    }

    send_packet(&mut host_socket, &RelayPacket::HostRegistered { room_id: room_id.clone() }).await?;

    let mut dummy_buf = [0u8; 1];
    tokio::select! {
        client_res = bridge_rx => {
            if let Ok(mut client_socket) = client_res {
                println!("[RELAY] Client joined room '{}'. Starting smart bridge...", room_id);
                send_packet(&mut host_socket, &RelayPacket::ClientConnected).await?;
                send_packet(&mut client_socket, &RelayPacket::SessionReady).await?;

                // === SMART BRIDGE (This is the main change) ===
                smart_bridge(host_socket, client_socket, room_id).await?;
            }
        }
        _ = host_socket.read(&mut dummy_buf) => {
            println!("[RELAY] Host disconnected. Cleaning room '{}']", room_id);
            rooms.lock().unwrap().remove(&room_id);
        }
    }
    Ok(())
}

async fn handle_client_connection(socket: TcpStream, room_id: String, rooms: RoomMap) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Same as before...
    let maybe_tx = {
        let mut map = rooms.lock().unwrap();
        map.remove(&room_id)
    };

    match maybe_tx {
        Some(tx) => { let _ = tx.send(socket); }
        None => {
            let mut s = socket;
            send_packet(&mut s, &RelayPacket::RoomNotFound).await?;
        }
    }
    Ok(())
}

// ==================== NEW: SMART BRIDGE ====================
async fn smart_bridge(mut host: TcpStream, mut client: TcpStream, room_id: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buf_h = [0u8; 4];
    let mut buf_c = [0u8; 4];

    loop {
        tokio::select! {
            // From Host → Client (mostly video frames)
            res_h = host.read_exact(&mut buf_h) => {
                if res_h.is_err() { break; }
                let len = u32::from_be_bytes(buf_h) as usize;
                let mut data = vec![0u8; len];
                if host.read_exact(&mut data).await.is_err() { break; }

                // Optional: You can inspect FramePacket here if needed

                if let Err(_) = forward_packet(&mut client, &data).await {
                    break;
                }
            }

            // From Client → Host (Input + Commands)
            res_c = client.read_exact(&mut buf_c) => {
                if res_c.is_err() { break; }
                let len = u32::from_be_bytes(buf_c) as usize;
                let mut data = vec![0u8; len];
                if client.read_exact(&mut data).await.is_err() { break; }

                // === THIS IS WHERE RELAY BECOMES DYNAMIC ===
                if let Ok(packet) = bincode::deserialize::<RelayPacket>(&data) {
                    if let RelayPacket::RelayCommand(cmd) = packet {
                        handle_relay_command(&cmd, &mut host, &room_id).await?;
                        continue; // don't forward to host if Relay handled it
                    }
                }

                // Normal forwarding (mouse input)
                if let Err(_) = forward_packet(&mut host, &data).await {
                    break;
                }
            }
        }
    }

    println!("[RELAY] Room '{}' closed.", room_id);
    Ok(())
}

async fn handle_relay_command(cmd: &Command, host: &mut TcpStream, room_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("[RELAY] Processing command for room {}: {:?}", room_id, cmd);
    
    match cmd {
        Command::Execute { cmd } => {
            // Forward to host
            let pkt = RelayPacket::RelayCommand(Command::Execute { cmd: cmd.clone() });
            let bytes = bincode::serialize(&pkt)?;
            let len = bytes.len() as u32;
            host.write_all(&len.to_be_bytes()).await?;
            host.write_all(&bytes).await?;
            println!("[RELAY] Forwarded Execute command: {}", cmd);
        }
        Command::PauseStream => {
            // You can add logic to tell host to pause
            println!("[RELAY] Pause requested");
        }
        _ => {}
    }
    Ok(())
}

async fn forward_packet(target: &mut TcpStream, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let len = data.len() as u32;
    target.write_all(&len.to_be_bytes()).await?;
    target.write_all(data).await?;
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
    Ok(bincode::deserialize(&buf)?)
}
