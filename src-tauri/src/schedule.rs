use crate::models::*;
use chrono::{DateTime, Days, LocalResult, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

pub fn resolve(local: NaiveDateTime, tz: Tz) -> Option<DateTime<Utc>> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(d) => Some(d.with_timezone(&Utc)),
        LocalResult::Ambiguous(a, _) => Some(a.with_timezone(&Utc)),
        LocalResult::None => None,
    }
}
pub fn validate(job: &mut Job) -> Result<()> {
    if job.recurrence == "immediate" {
        job.next_at = Some(now());
        return Ok(());
    }
    let tz: Tz = job
        .timezone
        .parse()
        .map_err(|_| "Use an IANA timezone, e.g. Asia/Kolkata".to_string())?;
    if !matches!(job.recurrence.as_str(), "once" | "daily" | "weekly") {
        return Err("Recurrence must be once, daily, or weekly".into());
    }
    let local = NaiveDateTime::parse_from_str(&job.local_time, "%Y-%m-%dT%H:%M")
        .map_err(|_| "Choose a valid local date and time".to_string())?;
    job.next_at = Some(
        resolve(local, tz)
            .ok_or("That local time does not exist due to daylight saving time")?
            .to_rfc3339(),
    );
    Ok(())
}
pub fn advance(job: &Job, after: DateTime<Utc>) -> Result<Option<String>> {
    if matches!(job.recurrence.as_str(), "once" | "immediate") {
        return Ok(None);
    }
    let tz: Tz = job.timezone.parse().map_err(|_| "Invalid timezone")?;
    let mut local = NaiveDateTime::parse_from_str(&job.local_time, "%Y-%m-%dT%H:%M")
        .map_err(|_| "Invalid schedule time")?;
    let step = if job.recurrence == "weekly" { 7 } else { 1 };
    // Jump near the current date, retaining the original wall-clock time and weekly cadence.
    let elapsed = (after.with_timezone(&tz).date_naive() - local.date())
        .num_days()
        .max(0) as u64;
    local = local
        .checked_add_days(Days::new((elapsed / step) * step))
        .ok_or("Schedule date overflow")?;
    for _ in 0..10 {
        if let Some(at) = resolve(local, tz)
            && at > after
        {
            return Ok(Some(at.to_rfc3339()));
        }
        local = local
            .checked_add_days(Days::new(step))
            .ok_or("Schedule date overflow")?;
    }
    Err("Cannot calculate next occurrence".into())
}

impl crate::runtime::Runtime {
    pub async fn job_tool(
        &self,
        run: &Execution,
        args: serde_json::Value,
    ) -> Result<serde_json::Value> {
        use serde_json::json;
        let action = args["action"].as_str().ok_or("Missing action")?;
        if action == "list" {
            return Ok(json!(
                self.db
                    .list::<Job>("job")?
                    .into_iter()
                    .filter(|j| j.agent_id == run.agent_id)
                    .collect::<Vec<_>>()
            ));
        }
        let key = format!(
            "{}:job:{}",
            run.id,
            args["request_id"]
                .as_str()
                .ok_or("Missing stable request_id")?
        );
        if let Ok(previous) = self.db.get::<serde_json::Value>("job_proposal", &key) {
            return Ok(previous);
        }
        let mut job: Job = if action == "create" {
            let mut job: Job =
                serde_json::from_value(args["job"].clone()).map_err(|e| e.to_string())?;
            job.id = id();
            job.agent_id = run.agent_id.clone();
            job.group_id = run.group_id.clone();
            job
        } else {
            self.db
                .get("job", args["job_id"].as_str().ok_or("Missing job_id")?)?
        };
        if job.agent_id != run.agent_id {
            return Err("Job belongs to another agent".into());
        }
        match action {
            "create" => {}
            "edit" => {
                let proposed: Job =
                    serde_json::from_value(args["job"].clone()).map_err(|e| e.to_string())?;
                job.name = proposed.name;
                job.prompt = proposed.prompt;
                job.timezone = proposed.timezone;
                job.local_time = proposed.local_time;
                job.recurrence = proposed.recurrence;
            }
            "pause" => job.paused = true,
            "delete" => {}
            _ => return Err("Unsupported job action".into()),
        }
        if job.name.trim().is_empty() || job.prompt.trim().is_empty() {
            return Err("Job needs a name and task".into());
        }
        if matches!(action, "create" | "edit") {
            validate(&mut job)?;
        }
        let approval_id = self
            .approve_record(
                &run.id,
                "schedule",
                json!({"action":action,"job":job,"working_folder":self.work_path(&run.agent_id)?}),
            )
            .await?;
        let _guard = self.mutations.lock().unwrap();
        if self.cancelled(&run.id) {
            return Err("Execution cancelled".into());
        }
        if let Ok(previous) = self.db.get::<serde_json::Value>("job_proposal", &key) {
            return Ok(previous);
        }
        if job.recurrence == "immediate" {
            job.next_at = Some(now());
        }
        let result = json!({"job_id":job.id,"action":action,"status":"approved"});
        self.db
            .apply_job(&job, action == "delete", &key, &result, &approval_id)?;
        self.notify();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dst_gap_and_overlap() {
        let tz: Tz = "America/New_York".parse().unwrap();
        let gap = NaiveDateTime::parse_from_str("2026-03-08T02:30", "%Y-%m-%dT%H:%M").unwrap();
        assert!(resolve(gap, tz).is_none());
        let overlap = NaiveDateTime::parse_from_str("2026-11-01T01:30", "%Y-%m-%dT%H:%M").unwrap();
        assert_eq!(
            resolve(overlap, tz).unwrap().to_rfc3339(),
            "2026-11-01T05:30:00+00:00"
        );
    }
    #[test]
    fn daily_uses_local_time() {
        let mut j = Job {
            timezone: "Asia/Kolkata".into(),
            local_time: "2026-10-25T09:00".into(),
            recurrence: "daily".into(),
            ..Default::default()
        };
        validate(&mut j).unwrap();
        assert_eq!(j.next_at.as_deref(), Some("2026-10-25T03:30:00+00:00"));
        let at = DateTime::parse_from_rfc3339(j.next_at.as_ref().unwrap())
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            advance(&j, at).unwrap().unwrap(),
            "2026-10-26T03:30:00+00:00"
        );
    }
}
