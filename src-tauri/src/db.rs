use crate::models::*;

use rusqlite::{Connection, OptionalExtension, params};

use serde::{Serialize, de::DeserializeOwned};

use serde_json::{Value, json};

use std::{path::Path, sync::Mutex};

pub struct Store(pub Mutex<Connection>);

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let c = Connection::open(path).map_err(|e| e.to_string())?;

        Self::init(c)
    }

    pub fn init(c: Connection) -> Result<Self> {
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
 CREATE TABLE IF NOT EXISTS migrations(version INTEGER PRIMARY KEY);
 CREATE TABLE IF NOT EXISTS entities(kind TEXT NOT NULL,id TEXT NOT NULL,data TEXT NOT NULL,PRIMARY KEY(kind,id));
 CREATE TABLE IF NOT EXISTS executions(id TEXT PRIMARY KEY,agent_id TEXT NOT NULL,conversation_id TEXT NOT NULL,group_id TEXT,job_id TEXT,chain_id TEXT NOT NULL,depth INTEGER NOT NULL,prompt TEXT NOT NULL,status TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,error TEXT);
 CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,execution_id TEXT NOT NULL,kind TEXT NOT NULL,data TEXT NOT NULL,created_at TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY,conversation_id TEXT NOT NULL,agent_id TEXT,execution_id TEXT,role TEXT NOT NULL,content TEXT NOT NULL,created_at TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS sessions(agent_id TEXT NOT NULL,conversation_id TEXT NOT NULL,provider TEXT NOT NULL,session_id TEXT NOT NULL,PRIMARY KEY(agent_id,conversation_id,provider));
 CREATE TABLE IF NOT EXISTS handoffs(execution_id TEXT NOT NULL,request_id TEXT NOT NULL,result TEXT NOT NULL,PRIMARY KEY(execution_id,request_id));
 CREATE INDEX IF NOT EXISTS events_run ON events(execution_id,seq);
 CREATE INDEX IF NOT EXISTS messages_conversation ON messages(conversation_id,created_at);
 CREATE INDEX IF NOT EXISTS executions_status ON executions(status,created_at);
 INSERT OR IGNORE INTO migrations VALUES(1);").map_err(|e|e.to_string())?;

        Ok(Self(Mutex::new(c)))
    }

    pub fn put<T: Serialize>(&self, kind: &str, id: &str, value: &T) -> Result<()> {
        let data = serde_json::to_string(value).map_err(|e| e.to_string())?;

        self.0.lock().unwrap().execute("INSERT INTO entities VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data",params![kind,id,data]).map_err(|e|e.to_string())?;

        Ok(())
    }

    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<T> {
        let s: String = self
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT data FROM entities WHERE kind=? AND id=?",
                params![kind, id],
                |r| r.get(0),
            )
            .map_err(|_| format!("{kind} not found: {id}"))?;

        serde_json::from_str(&s).map_err(|e| e.to_string())
    }

    pub fn list<T: DeserializeOwned>(&self, kind: &str) -> Result<Vec<T>> {
        let c = self.0.lock().unwrap();

        let mut q = c
            .prepare("SELECT data FROM entities WHERE kind=? ORDER BY rowid")
            .map_err(|e| e.to_string())?;

        let rows = q
            .query_map([kind], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        rows.map(|r| {
            serde_json::from_str(&r.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        })
        .collect()
    }

    pub fn delete(&self, kind: &str, id: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM entities WHERE kind=? AND id=?",
                params![kind, id],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn insert_run(&self, e: &Execution) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO executions VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    e.id,
                    e.agent_id,
                    e.conversation_id,
                    e.group_id,
                    e.job_id,
                    e.chain_id,
                    e.depth,
                    e.prompt,
                    e.status,
                    e.created_at,
                    e.updated_at,
                    e.error
                ],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn insert_handoff(
        &self,
        e: &Execution,
        m: &Message,
        key: &str,
        result: &Value,
    ) -> Result<()> {
        let mut c = self.0.lock().unwrap();

        let tx = c.transaction().map_err(|e| e.to_string())?;

        tx.execute(
            "INSERT INTO executions VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                e.id,
                e.agent_id,
                e.conversation_id,
                e.group_id,
                e.job_id,
                e.chain_id,
                e.depth,
                e.prompt,
                e.status,
                e.created_at,
                e.updated_at,
                e.error
            ],
        )
        .map_err(|e| e.to_string())?;

        tx.execute(
            "INSERT INTO messages VALUES(?,?,?,?,?,?,?)",
            params![
                m.id,
                m.conversation_id,
                m.agent_id,
                m.execution_id,
                m.role,
                m.content,
                m.created_at
            ],
        )
        .map_err(|e| e.to_string())?;

        tx.execute(
            "INSERT INTO entities VALUES('handoff',?,?)",
            params![key, result.to_string()],
        )
        .map_err(|e| e.to_string())?;

        tx.commit().map_err(|e| e.to_string())
    }

    pub fn runs(&self) -> Result<Vec<Execution>> {
        let c = self.0.lock().unwrap();

        let mut q = c
            .prepare("SELECT * FROM executions ORDER BY created_at DESC")
            .map_err(|e| e.to_string())?;

        let r = q
            .query_map([], |r| {
                Ok(Execution {
                    id: r.get(0)?,
                    agent_id: r.get(1)?,
                    conversation_id: r.get(2)?,
                    group_id: r.get(3)?,
                    job_id: r.get(4)?,
                    chain_id: r.get(5)?,
                    depth: r.get(6)?,
                    prompt: r.get(7)?,
                    status: r.get(8)?,
                    created_at: r.get(9)?,
                    updated_at: r.get(10)?,
                    error: r.get(11)?,
                })
            })
            .map_err(|e| e.to_string())?;

        r.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn run(&self, id: &str) -> Result<Execution> {
        self.runs()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or("Execution not found".into())
    }

    pub fn status(&self, id: &str, status: &str, error: Option<&str>) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute(
                "UPDATE executions SET status=?,updated_at=?,error=? WHERE id=?",
                params![status, now(), error, id],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn event(&self, id: &str, kind: &str, data: Value) -> Result<Event> {
        let c = self.0.lock().unwrap();

        let at = now();

        c.execute(
            "INSERT INTO events(execution_id,kind,data,created_at) VALUES(?,?,?,?)",
            params![id, kind, data.to_string(), at],
        )
        .map_err(|e| e.to_string())?;

        Ok(Event {
            seq: c.last_insert_rowid(),
            execution_id: id.into(),
            kind: kind.into(),
            data,
            created_at: at,
        })
    }

    pub fn events(&self, id: &str) -> Result<Vec<Event>> {
        let c = self.0.lock().unwrap();

        let mut q=c.prepare("SELECT seq,execution_id,kind,data,created_at FROM events WHERE execution_id=? ORDER BY seq").map_err(|e|e.to_string())?;

        let rows = q
            .query_map([id], |r| {
                Ok(Event {
                    seq: r.get(0)?,
                    execution_id: r.get(1)?,
                    kind: r.get(2)?,
                    data: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or(Value::Null),
                    created_at: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn message(&self, m: &Message) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO messages VALUES(?,?,?,?,?,?,?)",
                params![
                    m.id,
                    m.conversation_id,
                    m.agent_id,
                    m.execution_id,
                    m.role,
                    m.content,
                    m.created_at
                ],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn messages(&self, conversation: &str) -> Result<Vec<Message>> {
        let c = self.0.lock().unwrap();

        let mut q = c
            .prepare("SELECT * FROM messages WHERE conversation_id=? ORDER BY rowid")
            .map_err(|e| e.to_string())?;

        let r = q
            .query_map([conversation], |r| {
                Ok(Message {
                    id: r.get(0)?,
                    conversation_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    execution_id: r.get(3)?,
                    role: r.get(4)?,
                    content: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;

        r.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn session(&self, a: &str, c: &str, p: &str) -> Option<String> {
        self.0.lock().unwrap().query_row("SELECT session_id FROM sessions WHERE agent_id=? AND conversation_id=? AND provider=?",params![a,c,p],|r|r.get(0)).optional().ok().flatten()
    }

    pub fn set_session(&self, a: &str, c: &str, p: &str, s: &str) -> Result<()> {
        self.0.lock().unwrap().execute("INSERT INTO sessions VALUES(?,?,?,?) ON CONFLICT(agent_id,conversation_id,provider) DO UPDATE SET session_id=excluded.session_id",params![a,c,p,s]).map_err(|e|e.to_string())?;

        Ok(())
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

        for e in self.runs()?.into_iter().filter(|e| active(&e.status)) {
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
    matches!(s, "queued" | "running" | "waiting_approval")
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn persistence_and_recovery() {
        let db = Store::init(Connection::open_in_memory().unwrap()).unwrap();

        let a = Agent::default();

        db.put("agent", &a.id, &a).unwrap();

        assert_eq!(db.get::<Agent>("agent", &a.id).unwrap().id, a.id);

        assert!(db.session("a", "b", "claude").is_none());

        db.set_session("a", "b", "claude", "s").unwrap();

        assert!(db.session("other", "b", "claude").is_none());

        assert_eq!(db.session("a", "b", "claude"), Some("s".into()));
    }
}
