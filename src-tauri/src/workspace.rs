//! Managed file IO: all model-supplied paths are relative to an agent capability.
use crate::{models::*, runtime::Runtime};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, MetadataExt, OpenOptions};
use serde_json::{Value, json};
use std::{
    io::{Read, Seek, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tauri::State;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn relative(path: &str) -> Result<&Path> {
    let p = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || path.contains(':')
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Use a relative path without traversal".into());
    }
    Ok(p)
}
fn regular(file: &cap_std::fs::File) -> Result<()> {
    let meta = file.metadata().map_err(err)?;
    #[cfg(unix)]
    let single = meta.nlink() == 1;
    #[cfg(windows)]
    let single = meta.number_of_links() == Some(1);
    if !meta.is_file() || !single {
        return Err("Only regular, unlinked files are allowed".into());
    }
    Ok(())
}
fn checked(dir: &Dir, path: &str) -> Result<PathBuf> {
    let p = relative(path)?;
    let mut partial = PathBuf::new();
    for component in p.components() {
        partial.push(component);
        match dir.symlink_metadata(&partial) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err("Symbolic links are not allowed".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(err(e)),
        }
    }
    Ok(p.to_owned())
}
fn descend(root: &Dir, path: &Path, create: bool) -> Result<Dir> {
    let mut current = root.try_clone().map_err(err)?;
    for component in path.components() {
        if let Component::Normal(name) = component {
            if create {
                match current.create_dir(name) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(err(e)),
                }
            }
            current = current.open_dir_nofollow(name).map_err(err)?;
        } else {
            return Err("Invalid directory component".into());
        }
    }
    Ok(current)
}
fn parent(root: &Dir, path: &str, create: bool) -> Result<(Dir, std::ffi::OsString)> {
    let p = relative(path)?;
    let dir = descend(root, p.parent().unwrap_or(Path::new("")), create)?;
    Ok((dir, p.file_name().unwrap().to_owned()))
}
impl Runtime {
    pub fn import_work_file(&self, agent: &str, relative: &str, source: &Path) -> Result<()> {
        copy_verified(
            source,
            &self.work_dir(agent)?,
            crate::workspace::relative(relative)?,
        )
    }
    pub fn work_path(&self, agent: &str) -> Result<PathBuf> {
        uuid::Uuid::parse_str(agent).map_err(err)?;
        let root: String = self
            .db
            .get("settings", "working_root")
            .map_err(|_| "Choose a working folder in Settings before running agents")?;
        if !Path::new(&root).is_dir() {
            return Err("Working folder is unavailable; execution blocked".into());
        }
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).map_err(err)?;
        let relative = format!("agents/{agent}");
        checked(&dir, &relative)?;
        dir.create_dir_all(&relative).map_err(err)?;
        Ok(Path::new(&root).join(relative))
    }
    pub fn work_dir(&self, agent: &str) -> Result<Dir> {
        uuid::Uuid::parse_str(agent).map_err(err)?;
        let root: String = self
            .db
            .get("settings", "working_root")
            .map_err(|_| "Choose a working folder in Settings")?;
        let root = Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(err)?;
        descend(&root, Path::new(&format!("agents/{agent}")), true)
    }
    pub fn read_work(&self, agent: &str, path: &str) -> Result<Vec<u8>> {
        let dir = self.work_dir(agent)?;
        let (dir, leaf) = parent(&dir, path, false)?;
        let mut file = dir
            .open_with(
                leaf,
                OpenOptions::new().read(true).follow(FollowSymlinks::No),
            )
            .map_err(err)?;
        regular(&file)?;
        if file.metadata().map_err(err)?.len() > 50_000_000 {
            return Err("File exceeds 50 MB tool limit".into());
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(err)?;
        Ok(bytes)
    }
    pub fn write_work(&self, agent: &str, path: &str, bytes: &[u8]) -> Result<()> {
        let dir = self.work_dir(agent)?;
        let (dir, path) = parent(&dir, path, true)?;
        // New inode + rename avoids modifying a pre-existing hard link and truncation races.
        let tmp = format!(".opendots-{}", id());
        let mut file = dir
            .open_with(&tmp, OpenOptions::new().write(true).create_new(true))
            .map_err(err)?;
        let result = (|| {
            file.write_all(bytes).map_err(err)?;
            file.sync_all().map_err(err)?;
            dir.rename(&tmp, &dir, &path).map_err(err)
        })();
        if result.is_err() {
            let _ = dir.remove_file(&tmp);
        }
        result
    }
    pub async fn file_tool(&self, run: &Execution, args: Value) -> Result<Value> {
        let action = args["action"].as_str().ok_or("Missing action")?;
        let path = args["path"].as_str().unwrap_or("");
        let agent: Agent = self.db.get("agent", &run.agent_id)?;
        let policy = agent
            .policies
            .get(&format!("files:{action}"))
            .map(String::as_str)
            .unwrap_or(if matches!(action, "read" | "list") {
                "allow"
            } else {
                "ask"
            });
        match policy {"deny"=>return Err("File action denied".into()),"ask"=>self.approve(&run.id,"file",json!({"agent_id":agent.id,"action":action,"path":path,"content":args["content"],"destination":args["destination"],"recipient":args["recipient"]})).await?,_=>{}}
        if self.cancelled(&run.id) {
            return Err("Execution cancelled".into());
        }
        if !matches!(action, "read" | "list" | "share")
            && [path, args["destination"].as_str().unwrap_or("")]
                .iter()
                .any(|p| matches!(p.split('/').next(), Some("artifacts" | "inputs" | "shared")))
        {
            return Err(
                "Browser artifacts, imports and shared copies are read-only to file mutation tools"
                    .into(),
            );
        }
        let dir = self.work_dir(&agent.id)?;
        let result = match action {
            "list" => {
                let folder = if path.is_empty() {
                    PathBuf::from(".")
                } else {
                    checked(&dir, path)?
                };
                let mut out = vec![];
                for e in dir.read_dir(folder).map_err(err)?.take(1000) {
                    let e = e.map_err(err)?;
                    out.push(json!({"name":e.file_name().to_string_lossy(),"directory":e.file_type().map_err(err)?.is_dir()}));
                }
                json!(out)
            }
            "read" => {
                json!({"content":String::from_utf8(self.read_work(&agent.id,path)?).map_err(|_|"File is not UTF-8 text")?})
            }
            "write" => {
                let content = args["content"].as_str().ok_or("Missing content")?;
                if content.len() > 1_000_000 {
                    return Err("Text exceeds 1 MB".into());
                }
                self.write_work(&agent.id, path, content.as_bytes())?;
                json!({"path":path})
            }
            "mkdir" => {
                descend(&dir, relative(path)?, true)?;
                json!({"path":path})
            }
            "delete" => {
                let (dir, leaf) = parent(&dir, path, false)?;
                dir.remove_file(leaf).map_err(err)?;
                json!({"deleted":path})
            }
            "move" => {
                let target = args["destination"].as_str().ok_or("Missing destination")?;
                let (source, name) = parent(&dir, path, false)?;
                let (dest, target) = parent(&dir, target, false)?;
                source.rename(name, &dest, &target).map_err(err)?;
                json!({"path":target})
            }
            "share" => {
                let recipient = args["recipient"].as_str().ok_or("Missing recipient")?;
                let group: Group = self.db.get(
                    "group",
                    run.group_id.as_deref().ok_or("Sharing requires a group")?,
                )?;
                if !group.members.contains(&recipient.to_string()) {
                    return Err("Recipient is not in the group".into());
                }
                let bytes = self.read_work(&agent.id, path)?;
                let target = format!(
                    "shared/{}/{}",
                    id(),
                    relative(path)?.file_name().unwrap().to_string_lossy()
                );
                self.write_work(recipient, &target, &bytes)?;
                let record = json!({"id":id(),"from":agent.id,"to":recipient,"source":path,"path":target,"execution_id":run.id});
                self.db
                    .put("file_share", record["id"].as_str().unwrap(), &record)?;
                record
            }
            _ => return Err("Unsupported file action".into()),
        };
        self.event(
            &run.id,
            "file_action",
            json!({"action":action,"path":path,"result":result}),
        );
        Ok(result)
    }
}
#[tauri::command]
pub fn working_folder(rt: State<'_, Arc<Runtime>>) -> Option<String> {
    rt.db.get("settings", "working_root").ok()
}
#[tauri::command]
pub fn set_working_folder(rt: State<'_, Arc<Runtime>>, path: String) -> Result<()> {
    let _guard = rt.mutations.lock().unwrap();
    if rt.db.runs()?.iter().any(|r| crate::db::active(&r.status)) {
        return Err("Finish or cancel active work before changing the folder".into());
    }
    let root = Path::new(&path).canonicalize().map_err(err)?;
    let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).map_err(err)?;
    dir.create_dir_all("agents").map_err(err)?;
    // Changing a populated workspace requires an explicit move workflow, never silently replace it.
    if let Ok(old) = rt.db.get::<String>("settings", "working_root")
        && Path::new(&old) != root
    {
        return Err("A working folder is already configured. Moving an existing workspace is not supported yet.".into());
    }
    relocate_legacy(&rt, &root, &dir)?;
    rt.notify();
    Ok(())
}
// The destination is verified before the single metadata transaction activates it.
// Interrupted copies are safe to retry: matching files are reused, conflicting files stop import.
fn copy_verified(source: &Path, dest: &Dir, relative_path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(source).map_err(err)?;
    if meta.file_type().is_symlink() {
        return Err("Legacy artifact contains a symbolic link; import stopped".into());
    }
    if meta.is_dir() {
        let child = descend(dest, relative_path, true)?;
        for entry in std::fs::read_dir(source).map_err(err)? {
            let entry = entry.map_err(err)?;
            copy_verified(&entry.path(), &child, Path::new(&entry.file_name()))?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        return Err("Unsupported legacy artifact type".into());
    }
    let (parent, leaf) = parent(
        dest,
        relative_path.to_str().ok_or("Non-UTF8 artifact path")?,
        true,
    )?;
    let mut input = std::fs::File::open(source).map_err(err)?;
    if parent.symlink_metadata(&leaf).is_err() {
        let temp = format!(".import-{}", id());
        let mut output = parent
            .open_with(&temp, OpenOptions::new().write(true).create_new(true))
            .map_err(err)?;
        std::io::copy(&mut input, &mut output).map_err(err)?;
        output.sync_all().map_err(err)?;
        parent.rename(&temp, &parent, &leaf).map_err(err)?;
    }
    input.rewind().map_err(err)?;
    let mut output = parent
        .open_with(
            &leaf,
            OpenOptions::new().read(true).follow(FollowSymlinks::No),
        )
        .map_err(err)?;
    regular(&output)?;
    if input.metadata().map_err(err)?.len() != output.metadata().map_err(err)?.len() {
        return Err("Working-folder import found conflicting bytes; originals preserved".into());
    }
    let mut left = [0u8; 65536];
    let mut right = [0u8; 65536];
    loop {
        let n = input.read(&mut left).map_err(err)?;
        if n == 0 {
            break;
        }
        output.read_exact(&mut right[..n]).map_err(err)?;
        if left[..n] != right[..n] {
            return Err("Working-folder import verification failed; originals preserved".into());
        }
    }

    Ok(())
}
fn relocate_legacy(rt: &Runtime, root: &Path, dir: &Dir) -> Result<()> {
    let mut changes = vec![];
    for run in rt.db.runs()? {
        uuid::Uuid::parse_str(&run.agent_id).map_err(err)?;
        uuid::Uuid::parse_str(&run.id).map_err(err)?;
        let source = rt.root.join("browser-artifacts").join(&run.id);
        if source.exists() {
            copy_verified(
                &source,
                dir,
                Path::new(&format!("agents/{}/artifacts/{}", run.agent_id, run.id)),
            )?;
        }
    }
    for (key, mut record) in rt.db.entries("browser_upload")? {
        let agent = record["agent_id"].as_str().ok_or("Invalid upload owner")?;
        uuid::Uuid::parse_str(agent).map_err(err)?;
        uuid::Uuid::parse_str(&key).map_err(err)?;
        let source = Path::new(record["path"].as_str().ok_or("Invalid upload path")?);
        let relative = format!(
            "agents/{agent}/inputs/{key}/{}",
            source
                .file_name()
                .ok_or("Invalid upload filename")?
                .to_string_lossy()
        );
        copy_verified(source, dir, Path::new(&relative))?;
        record["path"] = json!(root.join(relative));
        changes.push(("browser_upload", key, record));
    }
    for (key, allowed) in rt.db.entries("browser_share")? {
        if allowed != true {
            continue;
        }
        let (recipient, reference) = key.split_once(':').ok_or("Invalid sharing grant")?;
        uuid::Uuid::parse_str(recipient).map_err(err)?;
        let (run, name) = reference
            .strip_prefix("browser:")
            .ok_or("Invalid sharing reference")?
            .split_once('/')
            .ok_or("Invalid sharing path")?;
        uuid::Uuid::parse_str(run).map_err(err)?;
        relative(name)?;
        let source = rt.root.join("browser-artifacts").join(run).join(name);
        let relative = format!("shared/{run}/{name}");
        copy_verified(
            &source,
            dir,
            Path::new(&format!("agents/{recipient}/{relative}")),
        )?;
        changes.push(("browser_share_copy", key, json!(relative)));
    }
    changes.push((
        "settings",
        "working_root".into(),
        json!(root.to_string_lossy()),
    ));
    changes.push((
        "metadata",
        "workspace_import".into(),
        json!({"completed_at":now(),"root":root,"originals_preserved":true}),
    ));
    rt.db.replace_documents(&changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_host_paths() {
        for p in ["../secret", "/tmp/a", "a/../b", "C:\\a", "a\\b", ""] {
            assert!(relative(p).is_err(), "{p}");
        }
    }
    #[test]
    fn legacy_media_copy_verifies_before_activation_and_is_retryable() {
        let rt = crate::tests::runtime();
        let a = Agent::default();
        rt.db.put("agent", &a.id, &a).unwrap();
        let run = rt
            .enqueue(
                SendInput {
                    thread_id: None,
                    agent_id: a.id.clone(),
                    group_id: None,
                    prompt: "fixture".into(),
                    job_id: None,
                },
                None,
            )
            .unwrap();
        let original = rt.root.join("browser-artifacts").join(&run.id);
        std::fs::create_dir_all(&original).unwrap();
        std::fs::write(original.join("proof.txt"), b"preserve me").unwrap();
        let new_root = rt.root.join("selected-root");
        std::fs::create_dir(&new_root).unwrap();
        let dir = Dir::open_ambient_dir(&new_root, cap_std::ambient_authority()).unwrap();
        let relative = format!("agents/{}/artifacts/{}/proof.txt", a.id, run.id);
        let (parent, leaf) = parent(&dir, &relative, true).unwrap();
        parent.write(&leaf, b"conflict").unwrap();
        assert!(relocate_legacy(&rt, &new_root, &dir).is_err());
        assert_ne!(
            rt.db.get::<String>("settings", "working_root").unwrap(),
            new_root.to_string_lossy()
        );
        parent.remove_file(&leaf).unwrap();
        parent.write(".import-interrupted", b"partial").unwrap();
        relocate_legacy(&rt, &new_root, &dir).unwrap();
        relocate_legacy(&rt, &new_root, &dir).unwrap();
        assert_eq!(
            std::fs::read(original.join("proof.txt")).unwrap(),
            b"preserve me"
        );
        assert_eq!(
            rt.read_work(&a.id, &format!("artifacts/{}/proof.txt", run.id))
                .unwrap(),
            b"preserve me"
        );
    }
}
