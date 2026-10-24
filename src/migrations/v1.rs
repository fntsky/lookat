use rusqlite::{Result, Transaction};
pub(super) fn up(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE devices (
        owner TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, platform TEXT NOT NULL, timezone TEXT NOT NULL,
        last_received_ms INTEGER NOT NULL, snapshot_json TEXT NOT NULL, snapshot_observed_ms INTEGER NOT NULL,
        snapshot_stream TEXT NOT NULL, snapshot_sequence INTEGER NOT NULL,
        PRIMARY KEY(owner,id)
    );
    CREATE TABLE batches (
        owner TEXT NOT NULL, device_id TEXT NOT NULL, id TEXT NOT NULL, stream_id TEXT NOT NULL,
        sequence INTEGER NOT NULL, body TEXT NOT NULL, received_at TEXT NOT NULL, interval_count INTEGER NOT NULL,
        PRIMARY KEY(owner,device_id,id), UNIQUE(owner,device_id,stream_id,sequence),
        FOREIGN KEY(owner,device_id) REFERENCES devices(owner,id)
    );
    CREATE TABLE intervals (
        owner TEXT NOT NULL, device_id TEXT NOT NULL, id TEXT NOT NULL, app_id TEXT NOT NULL,
        app_name TEXT NOT NULL, process_name TEXT NOT NULL, started_ms INTEGER NOT NULL, ended_ms INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL, body TEXT NOT NULL,
        PRIMARY KEY(owner,device_id,id), FOREIGN KEY(owner,device_id) REFERENCES devices(owner,id),
        CHECK(started_ms < ended_ms), CHECK(duration_ms BETWEEN 1 AND 60000)
    );
    CREATE INDEX intervals_time ON intervals(owner,device_id,started_ms,ended_ms);
    CREATE INDEX intervals_app ON intervals(owner,device_id,app_id,ended_ms);")
}
