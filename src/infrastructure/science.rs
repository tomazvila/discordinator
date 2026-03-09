//! Minimal science/telemetry event generation for anti-detection.
//!
//! The real Discord web client sends periodic POST requests to `/api/v10/science`
//! with batched analytics events. Complete absence of these is a detection signal.
//! This module generates a minimal set of plausible events.

use serde::Serialize;

/// A single science event.
#[derive(Debug, Clone, Serialize)]
pub struct ScienceEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub properties: serde_json::Value,
}

/// Tracks what events have been generated to avoid duplicates.
#[derive(Debug, Default)]
pub struct ScienceTracker {
    app_opened_sent: bool,
    events: Vec<ScienceEvent>,
}

impl ScienceTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record app opened event (sent once at startup).
    pub fn track_app_opened(&mut self) {
        if self.app_opened_sent {
            return;
        }
        self.app_opened_sent = true;
        self.events.push(ScienceEvent {
            event_type: "app_opened".to_string(),
            properties: serde_json::json!({
                "client_send_timestamp": timestamp_ms(),
                "client_track_timestamp": timestamp_ms(),
            }),
        });
    }

    /// Record channel opened event.
    pub fn track_channel_opened(&mut self, channel_id: u64, guild_id: Option<u64>) {
        self.events.push(ScienceEvent {
            event_type: "channel_opened".to_string(),
            properties: serde_json::json!({
                "channel_id": channel_id.to_string(),
                "guild_id": guild_id.map(|g| g.to_string()),
                "channel_type": i32::from(guild_id.is_none()),
                "client_send_timestamp": timestamp_ms(),
                "client_track_timestamp": timestamp_ms(),
            }),
        });
    }

    /// Record guild viewed event.
    pub fn track_guild_viewed(&mut self, guild_id: u64) {
        self.events.push(ScienceEvent {
            event_type: "guild_viewed".to_string(),
            properties: serde_json::json!({
                "guild_id": guild_id.to_string(),
                "client_send_timestamp": timestamp_ms(),
                "client_track_timestamp": timestamp_ms(),
            }),
        });
    }

    /// Drain accumulated events into a batch ready for POST.
    /// Returns `None` if no events are pending.
    pub fn drain_batch(&mut self) -> Option<serde_json::Value> {
        if self.events.is_empty() {
            return None;
        }
        let events: Vec<_> = self.events.drain(..).collect();
        Some(serde_json::json!({ "events": events }))
    }
}

fn timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_opened_sent_once() {
        let mut tracker = ScienceTracker::new();
        tracker.track_app_opened();
        tracker.track_app_opened(); // duplicate

        let batch = tracker.drain_batch().unwrap();
        let events = batch["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "app_opened");
    }

    #[test]
    fn channel_opened_event() {
        let mut tracker = ScienceTracker::new();
        tracker.track_channel_opened(12345, Some(67890));

        let batch = tracker.drain_batch().unwrap();
        let events = batch["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "channel_opened");
        assert_eq!(events[0]["properties"]["channel_id"], "12345");
        assert_eq!(events[0]["properties"]["guild_id"], "67890");
    }

    #[test]
    fn drain_clears_events() {
        let mut tracker = ScienceTracker::new();
        tracker.track_app_opened();
        assert!(tracker.drain_batch().is_some());
        assert!(tracker.drain_batch().is_none());
    }

    #[test]
    fn empty_tracker_returns_none() {
        let mut tracker = ScienceTracker::new();
        assert!(tracker.drain_batch().is_none());
    }

    #[test]
    fn multiple_events_batch() {
        let mut tracker = ScienceTracker::new();
        tracker.track_app_opened();
        tracker.track_channel_opened(111, None);
        tracker.track_guild_viewed(222);

        let batch = tracker.drain_batch().unwrap();
        let events = batch["events"].as_array().unwrap();
        assert_eq!(events.len(), 3);
    }
}
