use crate::problem::{Problem, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

fn optional<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    de: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(de).map(Some)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub process_name: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub executable: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub id: Uuid,
    pub name: String,
    pub platform: Platform,
    pub timezone: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Windows,
    Macos,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub name: String,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interval {
    pub id: Uuid,
    pub application: Application,
    pub started_at: String,
    pub ended_at: String,
    pub duration_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Snapshot {
    Active {
        observed_at: String,
        application: Application,
        since: String,
        session_ms: u64,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional"
        )]
        pid: Option<u32>,
    },
    Idle {
        observed_at: String,
    },
    Locked {
        observed_at: String,
    },
    Paused {
        observed_at: String,
    },
    NoWindow {
        observed_at: String,
    },
}
impl Snapshot {
    pub fn observed_at(&self) -> &str {
        match self {
            Self::Active { observed_at, .. }
            | Self::Idle { observed_at }
            | Self::Locked { observed_at }
            | Self::Paused { observed_at }
            | Self::NoWindow { observed_at } => observed_at,
        }
    }
    pub fn application(&self) -> Option<&Application> {
        match self {
            Self::Active { application, .. } => Some(application),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema_version: u32,
    pub batch_id: Uuid,
    pub stream_id: Uuid,
    pub sequence: u64,
    pub device: Device,
    pub agent: Agent,
    pub sent_at: String,
    pub intervals: Vec<Interval>,
    pub snapshot: Snapshot,
}

pub fn stamp(value: &str, field: &str) -> Result<i64> {
    let time = DateTime::parse_from_rfc3339(value)
        .map_err(|_| Problem::invalid(field, "时间戳必须是 UTC RFC3339 毫秒格式"))?;
    if time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string() != value
        || time.offset().local_minus_utc() != 0
    {
        return Err(Problem::invalid(field, "时间戳需使用 .000Z 毫秒格式"));
    }
    Ok(time.timestamp_millis())
}
fn bounded(value: &str, max: usize, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.chars().count() > max || value.chars().any(char::is_control)
    {
        return Err(Problem::invalid(
            field,
            format!("字段不能为空、含控制字符或超过 {max} 字符"),
        ));
    }
    Ok(())
}
impl Application {
    fn validate(&self, field: &str) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 128
            || !self.id.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:/-".contains(&b))
        {
            return Err(Problem::invalid(format!("{field}.id"), "无效的应用 ID"));
        }
        bounded(&self.name, 80, &format!("{field}.name"))?;
        bounded(&self.process_name, 260, &format!("{field}.process_name"))?;
        if let Some(path) = &self.executable {
            bounded(path, 4096, &format!("{field}.executable"))?;
        }
        Ok(())
    }
}
impl Batch {
    pub fn validate(&self, now: DateTime<Utc>) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Problem::invalid("schema_version", "不支持的协议版本"));
        }
        if self.sequence > 9_007_199_254_740_991 {
            return Err(Problem::invalid("sequence", "超过安全整数上限"));
        }
        bounded(&self.device.name, 80, "device.name")?;
        if self.device.timezone.len() > 64 || self.device.timezone.parse::<chrono_tz::Tz>().is_err()
        {
            return Err(Problem::invalid("device.timezone", "无效的 IANA 时区"));
        }
        let version = &self.agent.version;
        let (core, pre) = version
            .split_once('-')
            .map_or((version.as_str(), None), |(c, p)| (c, Some(p)));
        if self.agent.name != "lookat"
            || version.len() > 64
            || core.split('.').count() != 3
            || !core
                .split('.')
                .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            || pre.is_some_and(|p| {
                p.is_empty()
                    || !p
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
            })
        {
            return Err(Problem::invalid("agent", "无效的采集端版本"));
        }
        let sent = stamp(&self.sent_at, "sent_at")?;
        let observed = stamp(self.snapshot.observed_at(), "snapshot.observed_at")?;
        let oldest = now.timestamp_millis() - 7 * 86_400_000;
        if sent > now.timestamp_millis() + 120_000 || observed > sent || observed < oldest {
            return Err(Problem::invalid("sent_at", "批次时间超出允许范围"));
        }
        if self.intervals.len() > 20 {
            return Err(Problem::invalid("intervals", "每批最多 20 条区间"));
        }
        let mut previous_end = i64::MIN;
        let mut ids = std::collections::HashSet::new();
        for (i, interval) in self.intervals.iter().enumerate() {
            let field = format!("intervals[{i}]");
            interval
                .application
                .validate(&format!("{field}.application"))?;
            let start = stamp(&interval.started_at, &format!("{field}.started_at"))?;
            let end = stamp(&interval.ended_at, &format!("{field}.ended_at"))?;
            if interval.duration_ms == 0
                || interval.duration_ms > 60_000
                || start >= end
                || end > sent
                || start < oldest
                || (end - start - interval.duration_ms as i64).abs() > 1000
            {
                return Err(Problem::invalid(&field, "区间时间或时长无效"));
            }
            if start < previous_end || !ids.insert(interval.id) {
                return Err(Problem::invalid(&field, "区间必须有序、不重叠且 ID 唯一"));
            }
            previous_end = end;
        }
        if let Snapshot::Active {
            application,
            since,
            session_ms,
            pid,
            ..
        } = &self.snapshot
        {
            application.validate("snapshot.application")?;
            let since = stamp(since, "snapshot.since")?;
            if since > observed
                || *session_ms > 9_007_199_254_740_991
                || *session_ms as i128 > observed as i128 - since as i128 + 1000
                || *pid == Some(0)
            {
                return Err(Problem::invalid("snapshot", "快照起点或会话时长无效"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_examples_and_time_semantics_are_valid() {
        for input in [
            include_str!("../docs/examples/active-batch.json"),
            include_str!("../docs/examples/locked-heartbeat.json"),
        ] {
            let batch: Batch = serde_json::from_str(input).unwrap();
            batch
                .validate("2026-10-25T09:06:39.100Z".parse().unwrap())
                .unwrap();
        }
    }
    #[test]
    fn invalid_payloads_are_rejected() {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../docs/examples/active-batch.json")).unwrap();
        let now = "2026-10-25T09:06:24.100Z".parse().unwrap();
        for (pointer, value) in [
            ("/schema_version", serde_json::json!(2)),
            ("/sequence", serde_json::json!(9_007_199_254_740_992u64)),
            ("/device/timezone", serde_json::json!("Not/AZone")),
            ("/intervals/0/duration_ms", serde_json::json!(60001)),
            (
                "/intervals/0/ended_at",
                serde_json::json!("2026-10-25T09:06:08.000Z"),
            ),
            (
                "/snapshot/session_ms",
                serde_json::json!(9_007_199_254_740_991u64),
            ),
        ] {
            let mut changed = raw.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            let batch: Batch = serde_json::from_value(changed).unwrap();
            assert!(batch.validate(now).is_err(), "{pointer}");
        }
        for pointer in ["/snapshot/application/executable", "/snapshot/pid"] {
            let mut changed = raw.clone();
            if pointer.ends_with("executable") {
                changed["snapshot"]["application"]["executable"] = serde_json::Value::Null;
            } else {
                changed["snapshot"]["pid"] = serde_json::Value::Null;
            }
            assert!(serde_json::from_value::<Batch>(changed).is_err());
        }
        let mut unknown = raw;
        unknown["user_id"] = serde_json::json!("spoof");
        assert!(serde_json::from_value::<Batch>(unknown).is_err());
    }
}
