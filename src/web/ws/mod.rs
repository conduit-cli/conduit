//! WebSocket module for real-time agent communication.

mod handler;
mod messages;

#[cfg(test)]
mod tests;

pub use handler::{handle_websocket, SessionManager};
pub use messages::{ClientMessage, ServerMessage};
