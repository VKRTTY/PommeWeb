//! Browsers can't access the Discord desktop IPC socket.
pub enum DiscordPresence {}

impl DiscordPresence {
    pub fn start(_: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Discord desktop IPC is unavailable in browsers",
        )
        .into())
    }

    pub fn set_in_menu(&mut self, _: &str) {
        match *self {}
    }

    pub fn playing_multiplayer(&mut self, _: &str) {
        match *self {}
    }

    pub fn playing_singleplayer(&mut self, _: &str) {
        match *self {}
    }
}
