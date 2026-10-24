use crate::{
    problem::{Problem, Result},
    protocol::{Batch, Snapshot, stamp},
};
use axum::http::StatusCode;
use chrono::{DateTime, Days, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

pub type SharedStore = Arc<Mutex<Store>>;
pub struct Store {
    db: Connection,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub batch_id: String,
    pub status: &'static str,
    pub accepted_intervals: usize,
    pub duplicate_intervals: usize,
    pub received_at: String,
}
#[derive(Debug, Serialize)]
pub struct DeviceSummary {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub timezone: String,
    pub last_received_at: String,
    pub online: bool,
}
#[derive(Debug, Serialize)]
pub struct AppUsage {
    pub id: String,
    pub name: String,
    pub process: String,
    pub usage_ms: u64,
    pub total_ms: u64,
    pub last: String,
    pub current: bool,
}
#[derive(Debug, Serialize)]
pub struct CurrentApp {
    pub id: String,
    pub name: String,
    pub process: String,
    pub session_ms: u64,
}
#[derive(Debug, Serialize)]
pub struct Overview {
    pub server_now: String,
    pub date: String,
    pub period: String,
    pub timezone: String,
    pub current: Option<CurrentApp>,
    pub apps: Vec<AppUsage>,
    pub total_ms: u64,
    pub today_ms: u64,
    pub observed_at: String,
    pub stale: bool,
    pub state: String,
}

fn json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(Problem::internal)
}
fn iso(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn midnight(date: NaiveDate, tz: Tz) -> Result<i64> {
    let base = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| Problem::invalid("date", "无效日期"))?;
    // 某些 IANA 时区在午夜跳过一段墙钟时间，使用当天首个有效分钟。
    for minute in 0..=180 {
        if let Some(time) = tz
            .from_local_datetime(&(base + chrono::Duration::minutes(minute)))
            .earliest()
        {
            return Ok(time.timestamp_millis());
        }
    }
    Err(Problem::invalid("date", "该时区不存在此日期的有效午夜"))
}
fn portion(start: i64, end: i64, duration: u64, from: i64, to: i64) -> u64 {
    let left = start.max(from);
    let right = end.min(to);
    if right <= left {
        return 0;
    }
    // 前缀差保证相邻报表边界的整数毫秒之和等于原区间。
    let length = (end - start) as u128;
    ((duration as u128 * (right - start) as u128 / length)
        - (duration as u128 * (left - start) as u128 / length)) as u64
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(Problem::internal)?;
        }
        Self::from_connection(Connection::open(path)?)
    }
    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }
    fn from_connection(mut db: Connection) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        crate::migrations::migrate(&mut db)?;
        Ok(Self { db })
    }
    pub fn ingest(&mut self, owner: &str, batch: Batch, now: DateTime<Utc>) -> Result<Receipt> {
        // 已鉴权后才检查正文。重复批次先比较持久化正文，再做时效校验，允许重试确认。
        let body = json(&batch)?;
        let id = batch.batch_id.to_string();
        let device_id = batch.device.id.to_string();
        let stream = batch.stream_id.to_string();
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let previous:Option<(String,String,i64)>=tx.query_row("SELECT body,received_at,interval_count FROM batches WHERE owner=?1 AND device_id=?2 AND id=?3",params![owner,device_id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((previous, received, count)) = previous {
            if previous != body {
                return Err(Problem::conflict("同一 batch_id 的正文发生变化"));
            }
            return Ok(Receipt {
                schema_version: 1,
                batch_id: id,
                status: "duplicate",
                accepted_intervals: 0,
                duplicate_intervals: count as usize,
                received_at: received,
            });
        }
        batch.validate(now)?;
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM batches WHERE owner=?1 AND device_id=?2 AND stream_id=?3 AND sequence=?4)",params![owner,device_id,stream,batch.sequence as i64],|r|r.get::<_,bool>(0))? {return Err(Problem::conflict("同一 stream_id / sequence 已被其他批次使用"));}
        let observed = stamp(batch.snapshot.observed_at(), "snapshot.observed_at")?;
        let snapshot = json(&batch.snapshot)?;
        tx.execute("INSERT INTO devices(owner,id,name,platform,timezone,last_received_ms,snapshot_json,snapshot_observed_ms,snapshot_stream,snapshot_sequence)
          VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
          ON CONFLICT(owner,id) DO UPDATE SET name=excluded.name,platform=excluded.platform,timezone=excluded.timezone,last_received_ms=excluded.last_received_ms",
          params![owner,device_id,batch.device.name,json(&batch.device.platform)?.trim_matches('"'),batch.device.timezone,now.timestamp_millis(),snapshot,observed,stream,batch.sequence as i64])?;
        tx.execute("UPDATE devices SET snapshot_json=?3,snapshot_observed_ms=?4,snapshot_stream=?5,snapshot_sequence=?6
          WHERE owner=?1 AND id=?2 AND snapshot_observed_ms<?4 AND (snapshot_stream<>?5 OR snapshot_sequence<?6)",params![owner,device_id,snapshot,observed,stream,batch.sequence as i64])?;
        let mut duplicates = 0;
        for interval in &batch.intervals {
            let interval_id = interval.id.to_string();
            let encoded = json(interval)?;
            let previous: Option<String> = tx
                .query_row(
                    "SELECT body FROM intervals WHERE owner=?1 AND device_id=?2 AND id=?3",
                    params![owner, device_id, interval_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(previous) = previous {
                if previous != encoded {
                    return Err(Problem::conflict("同一 interval.id 的字段发生变化"));
                }
                duplicates += 1;
                continue;
            }
            let start = stamp(&interval.started_at, "started_at")?;
            let end = stamp(&interval.ended_at, "ended_at")?;
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM intervals WHERE owner=?1 AND device_id=?2 AND started_ms<?4 AND ended_ms>?3)",params![owner,device_id,start,end],|r|r.get::<_,bool>(0))? {return Err(Problem::conflict("同一设备的有效区间发生重叠"));}
            tx.execute("INSERT INTO intervals(owner,device_id,id,app_id,app_name,process_name,started_ms,ended_ms,duration_ms,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![owner,device_id,interval_id,interval.application.id,interval.application.name,interval.application.process_name,start,end,interval.duration_ms as i64,encoded])?;
        }
        let received = iso(now.timestamp_millis());
        tx.execute("INSERT INTO batches(owner,device_id,id,stream_id,sequence,body,received_at,interval_count) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![owner,device_id,id,stream,batch.sequence as i64,body,received,batch.intervals.len() as i64])?;
        tx.commit()?;
        Ok(Receipt {
            schema_version: 1,
            batch_id: id,
            status: "accepted",
            accepted_intervals: batch.intervals.len() - duplicates,
            duplicate_intervals: duplicates,
            received_at: received,
        })
    }
    pub fn devices(&self, owner: &str, now: DateTime<Utc>) -> Result<Vec<DeviceSummary>> {
        self.device_summaries(Some(owner), now)
    }
    pub fn public_devices(&self, now: DateTime<Utc>) -> Result<Vec<DeviceSummary>> {
        self.device_summaries(None, now)
    }
    fn device_summaries(
        &self,
        owner: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Vec<DeviceSummary>> {
        let mut statement=self.db.prepare("SELECT id,name,platform,timezone,last_received_ms FROM devices WHERE (?1 IS NULL OR owner=?1) AND (?1 IS NOT NULL OR id IN (SELECT id FROM devices GROUP BY id HAVING COUNT(*)=1)) ORDER BY last_received_ms DESC,id")?;
        Ok(statement
            .query_map([owner], |r| {
                let last: i64 = r.get(4)?;
                Ok(DeviceSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    platform: r.get(2)?,
                    timezone: r.get(3)?,
                    last_received_at: iso(last),
                    online: now.timestamp_millis() - last <= 45_000,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn public_overview(
        &self,
        device_id: &str,
        period: &str,
        date: Option<&str>,
        timezone: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Overview> {
        let mut statement = self
            .db
            .prepare("SELECT owner FROM devices WHERE id=?1 LIMIT 2")?;
        let owners = statement
            .query_map([device_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        match owners.as_slice() {
            [owner] => self.overview(owner, device_id, period, date, timezone, now),
            [] => Err(Problem::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "设备不存在",
            )),
            _ => Err(Problem::conflict(
                "设备标识在多个账户中重复，请为采集端配置独立的设备标识",
            )),
        }
    }
    pub fn overview(
        &self,
        owner: &str,
        device_id: &str,
        period: &str,
        date: Option<&str>,
        timezone: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Overview> {
        if !matches!(period, "today" | "week") {
            return Err(Problem::invalid("period", "仅支持 today 或 week"));
        }
        let device:Option<(String,String,i64)>=self.db.query_row("SELECT timezone,snapshot_json,last_received_ms FROM devices WHERE owner=?1 AND id=?2",params![owner,device_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (device_timezone, snapshot_json, last_received) =
            device.ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "not_found", "设备不存在"))?;
        let tz = timezone
            .unwrap_or(&device_timezone)
            .parse::<Tz>()
            .map_err(|_| Problem::invalid("timezone", "无效 IANA 时区"))?;
        let today = now.with_timezone(&tz).date_naive();
        let date = match date {
            Some(value) => NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .filter(|date| date.format("%Y-%m-%d").to_string() == value)
                .ok_or_else(|| Problem::invalid("date", "无效日期"))?,
            None => today,
        };
        let end_date = date
            .checked_add_days(Days::new(1))
            .ok_or_else(|| Problem::invalid("date", "日期超出范围"))?;
        let start_date = if period == "week" {
            date.checked_sub_days(Days::new(6))
                .ok_or_else(|| Problem::invalid("date", "日期超出范围"))?
        } else {
            date
        };
        let from = midnight(start_date, tz)?;
        let to = midnight(end_date, tz)?;
        let today_from = midnight(today, tz)?;
        let today_to = midnight(today.checked_add_days(Days::new(1)).unwrap(), tz)?;
        let snapshot: Snapshot = serde_json::from_str(&snapshot_json).map_err(Problem::internal)?;
        let observed = stamp(snapshot.observed_at(), "observed_at")?;
        let stale = now.timestamp_millis() - observed > 30_000
            || observed > now.timestamp_millis() + 5000
            || now.timestamp_millis() - last_received > 45_000;
        let mut apps = BTreeMap::<String, AppUsage>::new();
        let mut today_ms = 0;
        let mut statement=self.db.prepare("SELECT app_id,app_name,process_name,started_ms,ended_ms,duration_ms FROM intervals WHERE owner=?1 AND device_id=?2 ORDER BY ended_ms")?;
        let mut records = statement.query(params![owner, device_id])?;
        while let Some(row) = records.next()? {
            let id: String = row.get(0)?;
            let start: i64 = row.get(3)?;
            let end: i64 = row.get(4)?;
            let duration = row.get::<_, i64>(5)? as u64;
            let app = apps.entry(id.clone()).or_insert_with(|| AppUsage {
                id: id.clone(),
                name: String::new(),
                process: String::new(),
                usage_ms: 0,
                total_ms: 0,
                last: String::new(),
                current: false,
            });
            app.name = row.get(1)?;
            app.process = row.get(2)?;
            app.usage_ms += portion(start, end, duration, from, to);
            app.total_ms += duration;
            today_ms += portion(start, end, duration, today_from, today_to);
            app.last = DateTime::from_timestamp_millis(end)
                .unwrap()
                .with_timezone(&tz)
                .format("%H:%M")
                .to_string();
        }
        let current = if let Snapshot::Active {
            application,
            session_ms,
            ..
        } = &snapshot
        {
            let app = apps
                .entry(application.id.clone())
                .or_insert_with(|| AppUsage {
                    id: application.id.clone(),
                    name: application.name.clone(),
                    process: application.process_name.clone(),
                    usage_ms: 0,
                    total_ms: 0,
                    last: String::new(),
                    current: false,
                });
            app.current = !stale;
            app.last = DateTime::from_timestamp_millis(observed)
                .unwrap()
                .with_timezone(&tz)
                .format("%H:%M")
                .to_string();
            Some(CurrentApp {
                id: application.id.clone(),
                name: application.name.clone(),
                process: application.process_name.clone(),
                session_ms: *session_ms,
            })
        } else {
            None
        };
        let total_ms = apps.values().map(|app| app.usage_ms).sum();
        let mut apps: Vec<_> = apps.into_values().collect();
        apps.sort_by(|a, b| b.usage_ms.cmp(&a.usage_ms).then(a.id.cmp(&b.id)));
        let state = serde_json::to_value(&snapshot).map_err(Problem::internal)?["state"]
            .as_str()
            .unwrap_or("no_window")
            .to_owned();
        Ok(Overview {
            server_now: iso(now.timestamp_millis()),
            date: date.to_string(),
            period: period.to_owned(),
            timezone: tz.name().to_owned(),
            current,
            apps,
            total_ms,
            today_ms,
            observed_at: snapshot.observed_at().into(),
            stale,
            state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Interval, Snapshot};
    use uuid::Uuid;
    pub fn fixture() -> (Batch, DateTime<Utc>) {
        let batch: Batch =
            serde_json::from_str(include_str!("../docs/examples/active-batch.json")).unwrap();
        (batch, "2026-10-25T09:06:24.100Z".parse().unwrap())
    }
    fn next(batch: &Batch) -> Batch {
        let mut result = batch.clone();
        result.batch_id = Uuid::new_v4();
        result.sequence += 1;
        result
    }
    #[test]
    fn public_reads_include_devices_but_never_merge_colliding_owner_records() {
        let mut store = Store::memory().unwrap();
        let (batch, now) = fixture();
        let device = batch.device.id.to_string();
        store.ingest("alice", batch.clone(), now).unwrap();
        assert_eq!(store.public_devices(now).unwrap().len(), 1);
        assert_eq!(
            store
                .public_overview(&device, "today", None, None, now)
                .unwrap()
                .total_ms,
            15000
        );
        let mut other = batch.clone();
        other.device.id = uuid::Uuid::new_v4();
        store.ingest("bob", other, now).unwrap();
        assert_eq!(store.public_devices(now).unwrap().len(), 2);
        store.ingest("bob", batch, now).unwrap();
        assert_eq!(store.public_devices(now).unwrap().len(), 1);
        assert_eq!(
            store
                .public_overview(&device, "today", None, None, now)
                .unwrap_err()
                .status,
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn accepts_persists_deduplicates_and_isolates_owners() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lookat.sqlite3");
        let (batch, now) = fixture();
        let device = batch.device.id.to_string();
        {
            let mut store = Store::open(&path).unwrap();
            assert_eq!(
                store
                    .ingest("alice", batch.clone(), now)
                    .unwrap()
                    .accepted_intervals,
                1
            );
            let duplicate = store
                .ingest("alice", batch.clone(), now + chrono::Duration::days(8))
                .unwrap();
            assert_eq!(duplicate.status, "duplicate");
            assert_eq!(duplicate.duplicate_intervals, 1);
            let receipt = store.ingest("alice", next(&batch), now).unwrap();
            assert_eq!(receipt.accepted_intervals, 0);
            assert_eq!(receipt.duplicate_intervals, 1);
            let overview = store
                .overview("alice", &device, "today", None, None, now)
                .unwrap();
            assert_eq!(overview.server_now, iso(now.timestamp_millis()));
            assert_eq!(overview.total_ms, 15000);
            assert_eq!(overview.current.unwrap().session_ms, 872000);
            assert!(store.devices("bob", now).unwrap().is_empty());
            assert_eq!(
                store
                    .overview("bob", &device, "today", None, None, now)
                    .unwrap_err()
                    .status,
                StatusCode::NOT_FOUND
            );
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .overview("alice", &device, "today", None, None, now)
                .unwrap()
                .total_ms,
            15000
        );
        assert_eq!(
            store
                .db
                .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn conflicting_batches_and_sequences_never_overwrite() {
        let (batch, now) = fixture();
        let mut store = Store::memory().unwrap();
        store.ingest("alice", batch.clone(), now).unwrap();
        let mut changed = batch.clone();
        changed.device.name = "changed".into();
        assert_eq!(
            store.ingest("alice", changed, now).unwrap_err().status,
            StatusCode::CONFLICT
        );
        let mut same_sequence = batch.clone();
        same_sequence.batch_id = Uuid::new_v4();
        assert_eq!(
            store
                .ingest("alice", same_sequence, now)
                .unwrap_err()
                .status,
            StatusCode::CONFLICT
        );
        let mut same_interval = next(&batch);
        same_interval.intervals[0].application.name = "renamed".into();
        assert_eq!(
            store
                .ingest("alice", same_interval, now)
                .unwrap_err()
                .status,
            StatusCode::CONFLICT
        );
        assert_eq!(
            store.devices("alice", now).unwrap()[0].name,
            batch.device.name
        );
    }
    #[test]
    fn overlap_rolls_back_whole_batch_including_snapshot_and_metadata() {
        let (batch, now) = fixture();
        let mut store = Store::memory().unwrap();
        store.ingest("alice", batch.clone(), now).unwrap();
        let mut conflict = next(&batch);
        conflict.device.name = "must roll back".into();
        let application = batch.intervals[0].application.clone();
        conflict.intervals = vec![
            Interval {
                id: Uuid::new_v4(),
                application: application.clone(),
                started_at: "2026-10-25T09:05:00.000Z".into(),
                ended_at: "2026-10-25T09:05:01.000Z".into(),
                duration_ms: 1000,
            },
            Interval {
                id: Uuid::new_v4(),
                application,
                started_at: "2026-10-25T09:06:10.000Z".into(),
                ended_at: "2026-10-25T09:06:11.000Z".into(),
                duration_ms: 1000,
            },
        ];
        assert_eq!(
            store.ingest("alice", conflict, now).unwrap_err().status,
            StatusCode::CONFLICT
        );
        assert_eq!(
            store
                .db
                .query_row("SELECT COUNT(*) FROM intervals", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .db
                .query_row("SELECT COUNT(*) FROM batches", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            store.devices("alice", now).unwrap()[0].name,
            batch.device.name
        );
    }
    #[test]
    fn late_batches_do_not_replace_current_and_old_snapshots_are_stale() {
        let (batch, now) = fixture();
        let mut store = Store::memory().unwrap();
        store.ingest("alice", batch.clone(), now).unwrap();
        let mut fresh = next(&batch);
        fresh.sequence += 1;
        fresh.intervals.clear();
        fresh.sent_at = "2026-10-25T09:06:30.000Z".into();
        fresh.snapshot = Snapshot::Locked {
            observed_at: fresh.sent_at.clone(),
        };
        let later = now + chrono::Duration::seconds(10);
        store.ingest("alice", fresh.clone(), later).unwrap();
        let mut old = next(&batch);
        old.intervals.clear();
        store.ingest("alice", old, later).unwrap();
        let view = store
            .overview(
                "alice",
                &batch.device.id.to_string(),
                "today",
                None,
                None,
                later,
            )
            .unwrap();
        assert_eq!(view.state, "locked");
        assert!(view.current.is_none());
        let stale = store
            .overview(
                "alice",
                &batch.device.id.to_string(),
                "today",
                None,
                None,
                later + chrono::Duration::seconds(60),
            )
            .unwrap();
        assert!(stale.stale);
    }
    #[test]
    fn midnight_split_preserves_duration_and_dst_uses_calendar_boundaries() {
        let (mut batch, _) = fixture();
        batch.sent_at = "2026-10-25T16:00:00.500Z".into();
        batch.snapshot = Snapshot::Paused {
            observed_at: batch.sent_at.clone(),
        };
        batch.intervals[0].started_at = "2026-10-25T15:59:59.500Z".into();
        batch.intervals[0].ended_at = batch.sent_at.clone();
        batch.intervals[0].duration_ms = 1001;
        let now = "2026-10-25T16:00:00.600Z".parse().unwrap();
        let mut store = Store::memory().unwrap();
        store.ingest("alice", batch.clone(), now).unwrap();
        let yesterday = store
            .overview(
                "alice",
                &batch.device.id.to_string(),
                "today",
                Some("2026-10-25"),
                None,
                now,
            )
            .unwrap();
        let today = store
            .overview(
                "alice",
                &batch.device.id.to_string(),
                "today",
                Some("2026-10-26"),
                None,
                now,
            )
            .unwrap();
        assert_eq!((yesterday.total_ms, today.total_ms), (500, 501));
        let zone: Tz = "America/New_York".parse().unwrap();
        for (day, hours) in [("2026-03-08", 23), ("2026-11-01", 25)] {
            let date: NaiveDate = day.parse().unwrap();
            let from = midnight(date, zone).unwrap();
            let to = midnight(date + Days::new(1), zone).unwrap();
            assert_eq!(to - from, hours * 3_600_000);
        }
    }
    #[test]
    fn future_database_version_is_rejected_without_resetting() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 2).unwrap();
        assert!(Store::from_connection(db).is_err());
    }
}
