use rusqlite::Connection;
mod v1;
const LATEST_VERSION: u32 = 1;
pub fn migrate(db: &mut Connection) -> rusqlite::Result<()> {
    let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > LATEST_VERSION {
        return Err(rusqlite::Error::InvalidParameterName(
            "数据库版本高于当前程序".into(),
        ));
    }
    for version in version + 1..=LATEST_VERSION {
        let tx = db.transaction()?;
        match version {
            1 => v1::up(&tx)?,
            _ => unreachable!(),
        }
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}
