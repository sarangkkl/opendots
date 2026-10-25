use crate::models::*;
use polodb_core::{
    CollectionT, Database,
    bson::{self, Document, doc},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock, Weak},
};

pub struct Store {
    database: Arc<Database>,
    serial: Mutex<()>,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn document<T: Serialize>(id: &str, value: &T) -> Result<Document> {
    let data = bson::to_bson(value).map_err(err)?;
    let mut record = doc! {"_id":id,"format":1,"data":data.clone()};
    if let bson::Bson::Document(fields) = data {
        for key in [
            "conversation_id",
            "thread_id",
            "position",
            "execution_id",
            "seq",
            "status",
            "next_at",
            "created_at",
        ] {
            if let Some(value) = fields.get(key) {
                record.insert(key, value.clone());
            }
        }
    }
    Ok(record)
}
fn decode<T: DeserializeOwned>(d: Document) -> Result<T> {
    if d.get_i32("format").unwrap_or(1) != 1 {
        return Err("Unsupported document format; upgrade OpenDots".into());
    }
    bson::from_bson(d.get("data").cloned().ok_or("Missing document data")?).map_err(err)
}
impl Store {
    pub fn open(legacy: &Path) -> Result<Self> {
        let destination = legacy.with_extension("polodb");
        static OPEN: OnceLock<Mutex<HashMap<std::path::PathBuf, Weak<Database>>>> = OnceLock::new();
        let mut open = OPEN.get_or_init(Default::default).lock().unwrap();
        if let Some(database) = open.get(&destination).and_then(Weak::upgrade) {
            return Ok(Self {
                database,
                serial: Mutex::new(()),
            });
        }
        if !destination.exists() {
            let stage = legacy.with_extension(format!("import-{}", id()));
            let database = Database::open_path(&stage).map_err(err)?;
            let store = Self {
                database: Arc::new(database),
                serial: Mutex::new(()),
            };
            let manifest = if legacy.exists() {
                store.import_sqlite(legacy)?
            } else {
                json!({"source":"new installation","records":0})
            };
            store.put("metadata", "import", &manifest)?;
            drop(store);
            std::fs::write(
                stage.join("import-manifest.json"),
                serde_json::to_vec_pretty(&manifest).map_err(err)?,
            )
            .map_err(err)?;
            std::fs::rename(&stage, &destination).map_err(err)?;
        }
        let database = Arc::new(Database::open_path(&destination).map_err(err)?);
        open.insert(destination, Arc::downgrade(&database));
        let store = Self {
            database,
            serial: Mutex::new(()),
        };
        if store.get::<i32>("metadata", "index_layout") != Ok(2) {
            for kind in store.database.list_collection_names().map_err(err)? {
                if kind == "counters" {
                    continue;
                }
                for (key, value) in store.entries(&kind)? {
                    store.put(&kind, &key, &value)?;
                }
            }
            store.put("metadata", "index_layout", &2)?;
        }
        let _:Value=store.get("metadata","import").map_err(|e|format!("Document store activation manifest missing: {e}. Original SQLite remains available."))?;
        // Every collection has a unique _id. History queries use indexed document fields.
        for (collection, field) in [
            ("messages", "conversation_id"),
            ("messages", "thread_id"),
            ("messages", "position"),
            ("notification", "status"),
            ("events", "execution_id"),
            ("executions", "status"),
            ("job", "next_at"),
        ] {
            let mut keys = Document::new();
            keys.insert(field, 1);
            store
                .database
                .collection::<Document>(collection)
                .create_index(polodb_core::IndexModel {
                    keys,
                    options: None,
                })
                .map_err(err)?;
        }
        Ok(store)
    }
    fn import_sqlite(&self, source: &Path) -> Result<Value> {
        let snapshot = source.with_extension(format!("backup-{}.sqlite", id()));
        let connection = rusqlite::Connection::open_with_flags(
            source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(err)?;
        connection
            .execute("VACUUM INTO ?", [snapshot.to_string_lossy().as_ref()])
            .map_err(err)?;
        let c = rusqlite::Connection::open_with_flags(
            &snapshot,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(err)?;
        let mut counts = serde_json::Map::new();
        for table in [
            "entities",
            "executions",
            "events",
            "messages",
            "sessions",
            "handoffs",
        ] {
            let mut query = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .map_err(err)?;
            let columns: Vec<String> = query.column_names().iter().map(|s| s.to_string()).collect();
            let mut rows = query.query([]).map_err(err)?;
            let mut count = 0;
            while let Some(row) = rows.next().map_err(err)? {
                let mut data = serde_json::Map::new();
                for (i, key) in columns.iter().enumerate() {
                    use rusqlite::types::ValueRef;
                    let value = match row.get_ref(i).map_err(err)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(n) => json!(n),
                        ValueRef::Real(n) => json!(n),
                        ValueRef::Text(s) => json!(std::str::from_utf8(s).map_err(err)?),
                        ValueRef::Blob(_) => {
                            return Err(format!("Unexpected binary legacy record in {table}"));
                        }
                    };
                    data.insert(key.clone(), value);
                }
                let mut value = Value::Object(data);
                if table == "messages" {
                    value["position"] = json!(count + 1);
                }
                let (kind, key, payload) = match table {
                    "entities" => (
                        value["kind"]
                            .as_str()
                            .ok_or("Invalid entity kind")?
                            .to_string(),
                        value["id"].as_str().ok_or("Invalid entity id")?.to_string(),
                        serde_json::from_str(value["data"].as_str().ok_or("Invalid entity data")?)
                            .map_err(err)?,
                    ),
                    "sessions" => (
                        "legacy_sessions".into(),
                        format!(
                            "{}:{}:{}",
                            value["agent_id"], value["conversation_id"], value["provider"]
                        ),
                        value,
                    ),
                    "handoffs" => (
                        "legacy_handoffs".into(),
                        format!("{}:{}", value["execution_id"], value["request_id"]),
                        value,
                    ),
                    "events" => {
                        value["data"] = serde_json::from_str(
                            value["data"].as_str().ok_or("Invalid event data")?,
                        )
                        .map_err(err)?;
                        (table.into(), value["seq"].to_string(), value)
                    }
                    _ => (
                        table.into(),
                        value["id"].as_str().ok_or("Invalid record ID")?.to_string(),
                        value,
                    ),
                };
                self.put(&kind, &key, &payload)?;
                if self.get::<Value>(&kind, &key)? != payload {
                    return Err(format!("Import verification failed: {kind}/{key}"));
                }
                count += 1;
            }
            counts.insert(table.into(), json!(count));
        }
        // Validate historical relationships without requiring deleted agents to still exist.
        for m in self.list::<Message>("messages")? {
            if let Some(run) = m.execution_id {
                let _: Execution = self.get("executions", &run)?;
            }
        }
        for e in self.list::<Event>("events")? {
            if e.execution_id != "system" {
                let _: Execution = self.get("executions", &e.execution_id)?;
            }
        }
        Ok(
            json!({"format":1,"snapshot":snapshot,"counts":counts,"verified":true,"provider_sessions":"retained as legacy; fresh sessions required"}),
        )
    }
    pub fn put<T: Serialize>(&self, kind: &str, id: &str, value: &T) -> Result<()> {
        let _guard = self.serial.lock().unwrap();
        let tx = self.database.start_transaction().map_err(err)?;
        let coll = tx.collection::<Document>(kind);
        coll.delete_one(doc! {"_id":id}).map_err(err)?;
        coll.insert_one(document(id, value)?).map_err(err)?;
        tx.commit().map_err(err)
    }
    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<T> {
        decode(
            self.database
                .collection::<Document>(kind)
                .find_one(doc! {"_id":id})
                .map_err(err)?
                .ok_or_else(|| format!("{kind} not found: {id}"))?,
        )
    }
    pub fn list<T: DeserializeOwned>(&self, kind: &str) -> Result<Vec<T>> {
        self.query(kind, doc! {}, None)
    }
    fn query<T: DeserializeOwned>(
        &self,
        kind: &str,
        filter: Document,
        limit: Option<u64>,
    ) -> Result<Vec<T>> {
        let coll = self.database.collection::<Document>(kind);
        let mut query = coll.find(filter);
        if let Some(limit) = limit {
            query = query.limit(limit);
        }
        query
            .run()
            .map_err(err)?
            .map(|r| decode(r.map_err(err)?))
            .collect()
    }
    pub fn delete(&self, kind: &str, id: &str) -> Result<()> {
        self.database
            .collection::<Document>(kind)
            .delete_one(doc! {"_id":id})
            .map_err(err)?;
        Ok(())
    }
    pub fn insert_handoff(
        &self,
        e: &Execution,
        m: &Message,
        key: &str,
        result: &Value,
    ) -> Result<()> {
        let tx = self.database.start_transaction().map_err(err)?;
        tx.collection::<Document>("executions")
            .insert_one(document(&e.id, e)?)
            .map_err(err)?;
        tx.collection::<Document>("messages")
            .insert_one(self.message_document(&tx, m)?)
            .map_err(err)?;
        tx.collection::<Document>("handoff")
            .insert_one(document(key, result)?)
            .map_err(err)?;
        tx.commit().map_err(err)
    }
    pub fn runs(&self) -> Result<Vec<Execution>> {
        let mut runs: Vec<Execution> = self.list("executions")?;
        runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(runs)
    }
    pub fn run(&self, id: &str) -> Result<Execution> {
        self.get("executions", id)
    }
    pub fn status(&self, id: &str, status: &str, error: Option<&str>) -> Result<()> {
        let mut run = self.run(id)?;
        run.status = status.into();
        run.updated_at = now();
        run.error = error.map(str::to_string);
        self.put("executions", id, &run)
    }
    pub fn event(&self, id: &str, kind: &str, data: Value) -> Result<Event> {
        let _guard = self.serial.lock().unwrap();
        let tx = self.database.start_transaction().map_err(err)?;
        let meta = tx.collection::<Document>("counters");
        let previous = meta
            .find_one(doc! {"_id":"events"})
            .map_err(err)?
            .and_then(|d| d.get_i64("seq").ok())
            .unwrap_or_else(|| {
                self.list::<Event>("events")
                    .unwrap_or_default()
                    .iter()
                    .map(|e| e.seq)
                    .max()
                    .unwrap_or(0)
            });
        let e = Event {
            seq: previous + 1,
            execution_id: id.into(),
            kind: kind.into(),
            data,
            created_at: now(),
        };
        meta.delete_one(doc! {"_id":"events"}).map_err(err)?;
        meta.insert_one(doc! {"_id":"events","seq":e.seq})
            .map_err(err)?;
        tx.collection::<Document>("events")
            .insert_one(document(&e.seq.to_string(), &e)?)
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(e)
    }
    pub fn events(&self, id: &str) -> Result<Vec<Event>> {
        let mut events: Vec<Event> = self.query("events", doc! {"execution_id":id}, None)?;
        events.sort_by_key(|e| e.seq);
        Ok(events)
    }
    pub fn message(&self, m: &Message) -> Result<()> {
        let tx = self.database.start_transaction().map_err(err)?;
        tx.collection::<Document>("messages")
            .insert_one(self.message_document(&tx, m)?)
            .map_err(err)?;
        tx.commit().map_err(err)
    }
    fn message_document(&self, tx: &polodb_core::Transaction, m: &Message) -> Result<Document> {
        let counters = tx.collection::<Document>("counters");
        let next = counters
            .find_one(doc! {"_id":"messages"})
            .map_err(err)?
            .and_then(|d| d.get_i64("seq").ok())
            .unwrap_or_else(|| {
                self.list::<Message>("messages")
                    .unwrap_or_default()
                    .iter()
                    .map(|m| m.position)
                    .max()
                    .unwrap_or(0)
            })
            + 1;
        counters.delete_one(doc! {"_id":"messages"}).map_err(err)?;
        counters
            .insert_one(doc! {"_id":"messages","seq":next})
            .map_err(err)?;
        if let Some(parent) = m.thread_id.as_ref() {
            let messages = tx.collection::<Document>("messages");
            if let Some(record) = messages.find_one(doc! {"_id":parent}).map_err(err)? {
                let mut root: Message = decode(record)?;
                root.reply_count += 1;
                messages.delete_one(doc! {"_id":parent}).map_err(err)?;
                messages.insert_one(document(parent, &root)?).map_err(err)?;
            }
        }
        let mut m = m.clone();
        m.position = next;
        document(&m.id, &m)
    }
    pub fn messages(&self, c: &str) -> Result<Vec<Message>> {
        let mut messages: Vec<Message> =
            self.query("messages", doc! {"conversation_id":c}, None)?;
        messages.sort_by_key(|m| m.position);
        Ok(messages)
    }
    pub fn session(&self, a: &str, c: &str, p: &str) -> Option<String> {
        self.get("sessions", &format!("{a}:{c}:{p}")).ok()
    }
    pub fn set_session(&self, a: &str, c: &str, p: &str, s: &str) -> Result<()> {
        self.put("sessions", &format!("{a}:{c}:{p}"), &s)
    }
    pub fn recover(&self) -> Result<()> {
        for mut occurrence in self.list::<Value>("occurrence")? {
            if occurrence["status"] == "dispatching" {
                occurrence["status"] = json!("interrupted");
                if let Some(id) = occurrence["id"].as_str() {
                    self.put("occurrence", id, &occurrence)?;
                }
            }
        }

        for e in self
            .runs()?
            .into_iter()
            .filter(|e| active(&e.status) && e.status != "waiting_user")
        {
            self.status(
                &e.id,
                "interrupted",
                Some("Application stopped before execution finished"),
            )?;

            self.event(
                &e.id,
                "status",
                json!({
                "status":"interrupted"}
                ),
            )?;
        }

        for mut a in self.list::<Approval>("approval")? {
            if a.status == "pending" {
                a.status = "expired".into();

                self.put("approval", &a.id, &a)?;
            }
        }

        Ok(())
    }
}

pub fn active(s: &str) -> bool {
    matches!(
        s,
        "queued" | "running" | "waiting_approval" | "waiting_user"
    )
}

impl Store {
    pub fn chat_transaction(&self, key: &str, result: &crate::chat::ChatResult) -> Result<()> {
        let tx = self.database.start_transaction().map_err(err)?;
        tx.collection::<Document>("dispatch")
            .insert_one(document(key, result)?)
            .map_err(err)?;
        tx.collection::<Document>("messages")
            .insert_one(self.message_document(&tx, &result.message)?)
            .map_err(err)?;
        for run in &result.executions {
            tx.collection::<Document>("executions")
                .insert_one(document(&run.id, run)?)
                .map_err(err)?;
        }
        if let Some(reply) = result.message.reply_to.as_ref()
            && let Ok(mut note) = self.get::<Value>("notification", reply)
            && note["status"] == "waiting"
            && !result.executions.is_empty()
        {
            note["status"] = json!("answered");
            note["read"] = json!(true);
            let c = tx.collection::<Document>("notification");
            c.delete_one(doc! {"_id":reply}).map_err(err)?;
            c.insert_one(document(reply, &note)?).map_err(err)?;
            if let Some(source) = note["execution_id"].as_str() {
                let mut run = self.run(source)?;
                run.status = "continued".into();
                run.updated_at = now();
                let c = tx.collection::<Document>("executions");
                c.delete_one(doc! {"_id":source}).map_err(err)?;
                c.insert_one(document(source, &run)?).map_err(err)?;
            }
        }
        tx.commit().map_err(err)
    }
    pub fn history(
        &self,
        conversation: &str,
        before: Option<&str>,
        thread: Option<&str>,
    ) -> Result<Vec<Message>> {
        let filter = doc! {"conversation_id":conversation};
        let before = before.map(str::parse::<i64>).transpose().map_err(err)?;
        let coll = self.database.collection::<Document>("messages");
        // PoloDB 5.3.0 indexed $match followed by $sort can lose rows.
        // Stream the indexed cursor and retain only the newest page.
        let mut page = std::collections::BTreeMap::new();
        for row in coll.find(filter).run().map_err(err)? {
            let message: Message = decode(row.map_err(err)?)?;
            if thread.is_none() && message.thread_id.is_some() {
                continue;
            }
            if before.is_some_and(|p| message.position >= p)
                || thread
                    .is_some_and(|t| message.thread_id.as_deref() != Some(t) && message.id != t)
            {
                continue;
            }
            page.insert(message.position, message);
            if page.len() > 50 {
                page.pop_first();
            }
        }
        Ok(page.into_values().collect())
    }
}
impl Store {
    pub fn apply_job(
        &self,
        job: &Job,
        delete: bool,
        key: &str,
        result: &Value,
        approval_id: &str,
    ) -> Result<()> {
        let tx = self.database.start_transaction().map_err(err)?;
        let approvals = tx.collection::<Document>("approval");
        let mut approval: Approval = decode(
            approvals
                .find_one(doc! {"_id":approval_id})
                .map_err(err)?
                .ok_or("Schedule approval not found")?,
        )?;
        if approval.status != "approved"
            || approval.kind != "schedule"
            || approval.payload["job"]["id"] != job.id
        {
            return Err("Schedule activation lacks matching approval".into());
        }
        approval.status = "applied".into();
        approvals
            .delete_one(doc! {"_id":approval_id})
            .map_err(err)?;
        approvals
            .insert_one(document(approval_id, &approval)?)
            .map_err(err)?;

        tx.collection::<Document>("job_proposal")
            .insert_one(document(key, result)?)
            .map_err(err)?;
        let jobs = tx.collection::<Document>("job");
        jobs.delete_one(doc! {"_id":&job.id}).map_err(err)?;
        if !delete {
            jobs.insert_one(document(&job.id, job)?).map_err(err)?;
        }
        tx.commit().map_err(err)
    }
}
impl Store {
    pub fn replace_documents(&self, items: &[(&str, String, Value)]) -> Result<()> {
        let tx = self.database.start_transaction().map_err(err)?;
        for (kind, id, value) in items {
            let c = tx.collection::<Document>(kind);
            c.delete_one(doc! {"_id":id}).map_err(err)?;
            c.insert_one(document(id, value)?).map_err(err)?;
        }
        tx.commit().map_err(err)
    }
    pub fn entries(&self, kind: &str) -> Result<Vec<(String, Value)>> {
        self.database
            .collection::<Document>(kind)
            .find(doc! {})
            .run()
            .map_err(err)?
            .map(|r| {
                let d = r.map_err(err)?;
                let key = d.get_str("_id").map_err(err)?.to_string();
                Ok((key, decode(d)?))
            })
            .collect()
    }
}

impl Store {
    pub fn scheduled_occurrence(
        &self,
        job: &Job,
        key: &str,
        record: &Value,
        prepared: Option<&(Execution, Message)>,
    ) -> Result<()> {
        let tx = self.database.start_transaction().map_err(err)?;
        tx.collection::<Document>("occurrence")
            .insert_one(document(key, record)?)
            .map_err(err)?;
        if let Some((run, message)) = prepared {
            tx.collection::<Document>("executions")
                .insert_one(document(&run.id, run)?)
                .map_err(err)?;
            tx.collection::<Document>("messages")
                .insert_one(self.message_document(&tx, message)?)
                .map_err(err)?;
        }
        let jobs = tx.collection::<Document>("job");
        jobs.delete_one(doc! {"_id":&job.id}).map_err(err)?;
        jobs.insert_one(document(&job.id, job)?).map_err(err)?;
        tx.commit().map_err(err)
    }
}

impl Store {
    pub fn agent_post(
        &self,
        run: &Execution,
        message: &Message,
        key: &str,
        result: &Value,
        notification: Option<&Value>,
        waiting: bool,
    ) -> Result<()> {
        let tx = self.database.start_transaction().map_err(err)?;
        tx.collection::<Document>("agent_post")
            .insert_one(document(key, result)?)
            .map_err(err)?;
        tx.collection::<Document>("messages")
            .insert_one(self.message_document(&tx, message)?)
            .map_err(err)?;
        if let Some(note) = notification {
            tx.collection::<Document>("notification")
                .insert_one(document(&message.id, note)?)
                .map_err(err)?;
        }
        if waiting {
            let mut run = run.clone();
            run.status = "waiting_user".into();
            run.updated_at = now();
            let c = tx.collection::<Document>("executions");
            c.delete_one(doc! {"_id":&run.id}).map_err(err)?;
            c.insert_one(document(&run.id, &run)?).map_err(err)?;
        }
        tx.commit().map_err(err)
    }
}

impl Store {
    pub fn event_page(&self, execution: &str, before: Option<i64>) -> Result<Vec<Event>> {
        let mut page = std::collections::BTreeMap::new();
        for row in self
            .database
            .collection::<Document>("events")
            .find(doc! {"execution_id":execution})
            .run()
            .map_err(err)?
        {
            let event: Event = decode(row.map_err(err)?)?;
            if before.is_some_and(|seq| event.seq >= seq) {
                continue;
            }
            page.insert(event.seq, event);
            if page.len() > 200 {
                page.pop_first();
            }
        }
        Ok(page.into_values().collect())
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;
    #[test]
    fn imports_verifies_and_preserves_legacy() {
        let root = std::env::temp_dir().join(format!("opendots-import-{}", id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("legacy.sqlite");
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(include_str!("legacy-fixture.sql")).unwrap();
        let agent = Agent::default();
        c.execute(
            "INSERT INTO entities VALUES('agent',?,?)",
            rusqlite::params![agent.id, serde_json::to_string(&agent).unwrap()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO sessions VALUES('a','b','claude','old-session')",
            [],
        )
        .unwrap();
        drop(c);
        let db = Store::open(&path).unwrap();
        assert_eq!(db.get::<Agent>("agent", &agent.id).unwrap().id, agent.id);
        assert!(db.session("a", "b", "claude").is_none());
        assert_eq!(db.list::<Value>("legacy_sessions").unwrap().len(), 1);
        let manifest: Value = db.get("metadata", "import").unwrap();
        assert_eq!(manifest["counts"]["entities"], 1);
        assert!(Path::new(manifest["snapshot"].as_str().unwrap()).exists());
        assert!(path.exists());
        drop(db);
        let db = Store::open(&path).unwrap();
        assert_eq!(db.list::<Agent>("agent").unwrap().len(), 1);
    }
    #[test]
    fn failed_import_never_activates_empty_database() {
        let root = std::env::temp_dir().join(format!("opendots-import-{}", id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("legacy.sqlite");
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(include_str!("legacy-fixture.sql")).unwrap();
        c.execute(
            "INSERT INTO entities VALUES('agent','bad','broken JSON')",
            [],
        )
        .unwrap();
        drop(c);
        assert!(Store::open(&path).is_err());
        assert!(!path.with_extension("polodb").exists());
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute("UPDATE entities SET data='{}'", []).unwrap();
        drop(c);
        assert!(Store::open(&path).is_ok());
    }
    #[test]
    fn history_cursor_keeps_equal_timestamps() {
        let rt = crate::tests::runtime();
        for i in 0..125 {
            rt.db
                .message(&Message {
                    reply_count: 0,
                    position: 0,
                    id: id(),
                    conversation_id: "test".into(),
                    thread_id: None,
                    reply_to: None,
                    mentions: vec![],
                    attachments: vec![],
                    agent_id: None,
                    execution_id: None,
                    role: "user".into(),
                    content: i.to_string(),
                    created_at: "same timestamp".into(),
                })
                .unwrap();
        }
        let first = rt.db.history("test", None, None).unwrap();
        assert_eq!(first.len(), 50);
        assert_eq!(first.last().unwrap().content, "124");
        let second = rt
            .db
            .history("test", Some(&first[0].position.to_string()), None)
            .unwrap();
        assert_eq!(second.len(), 50);
        assert_eq!(second.last().unwrap().content, "74");
        let last = rt
            .db
            .history("test", Some(&second[0].position.to_string()), None)
            .unwrap();
        assert_eq!(last.len(), 25);
    }
}
