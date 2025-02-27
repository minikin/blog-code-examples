use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A serializable representation of a timestamp
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TimeStamp {
    /// Seconds since Unix epoch
    pub seconds: u64,
    /// Nanoseconds part
    pub nanos: u32,
}

impl TimeStamp {
    /// Create a new timestamp from the current system time
    ///
    /// # Panics
    ///
    /// Panics if the system time is before the Unix epoch.
    #[must_use]
    pub fn now() -> Self {
        let now = SystemTime::now();
        #[allow(clippy::expect_used)]
        let duration = now.duration_since(UNIX_EPOCH).expect("System time is before UNIX epoch");

        Self { seconds: duration.as_secs(), nanos: duration.subsec_nanos() }
    }

    /// Convert to an instant
    #[allow(dead_code)]
    #[must_use]
    pub fn to_instant(&self) -> Instant {
        // This is an approximation since we can't create Instant directly
        Instant::now()
    }
}

/// A serializable wrapper around Instant
#[derive(Debug, Clone)]
pub struct SerializableInstant(Instant);

impl SerializableInstant {
    /// Create a new instance with the current time
    #[must_use]
    pub fn now() -> Self {
        Self(Instant::now())
    }

    /// Get the elapsed time since this instant was created
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }

    /// Get the underlying Instant
    #[must_use]
    pub fn inner(&self) -> &Instant {
        &self.0
    }
}

impl Serialize for SerializableInstant {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // We just serialize a timestamp when it was created
        let timestamp = TimeStamp::now();
        timestamp.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SerializableInstant {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // We deserialize the timestamp but discard it and create a new Instant
        let _timestamp = TimeStamp::deserialize(deserializer)?;
        Ok(Self::now())
    }
}
