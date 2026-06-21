pub mod protocol {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum Command {
        NextSlide,
        PrevSlide,
        MovePointer { x: f32, y: f32 },
        Authenticate { token: String },
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum FramePacket {
        VideoFrame(Vec<u8>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum InputPacket {
        MouseMove { x_percent: f32, y_percent: f32 },
        MouseClick { button: u8 },
    }

    // --- RENDEZVOUS / CLOUD RELAY PROTOCOL ---
    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum RelayPacket {
        RegisterHost { room_id: String },
        HostRegistered { room_id: String },
        ConnectToHost { room_id: String },
        ClientConnected,
        SessionReady,
        RoomNotFound,
    }
}
