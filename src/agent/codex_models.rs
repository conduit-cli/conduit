//! Codex's app-server catalog, refreshed off the UI thread on first use and hourly.
//! Unknown response fields (including new reasoning effort names) are deliberately
//! ignored so a newer CLI does not require updating Conduit's protocol dependency.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::{AgentType, ModelInfo, ModelRegistry};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogModel {
    model: String,
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    is_default: bool,
    #[serde(default)]
    hidden: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelPage {
    data: Vec<CatalogModel>,
    next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cache {
    binary: PathBuf,
    models: Vec<CatalogModel>,
}

#[derive(Default)]
struct Catalog {
    binary: Option<PathBuf>,
    models: Vec<CatalogModel>,
    refreshing: bool,
    next_refresh: Option<Instant>,
}

impl Catalog {
    fn complete_refresh(&mut self, result: io::Result<Vec<CatalogModel>>) -> bool {
        self.refreshing = false;
        match result {
            Ok(models) => {
                self.models = models;
                self.next_refresh = Some(Instant::now() + Duration::from_secs(3600));
                true
            }
            Err(error) => {
                tracing::warn!(%error, "Codex model discovery failed; retaining previous catalog");
                self.next_refresh = Some(Instant::now() + Duration::from_secs(60));
                false
            }
        }
    }
}

fn save_cache(path: &Path, cache: &Cache) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing cache directory"))?;
    std::fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec(cache)?;
    // Persist atomically so an interrupted write keeps the old catalog.
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn catalog() -> &'static Mutex<Catalog> {
    static CATALOG: OnceLock<Mutex<Catalog>> = OnceLock::new();
    CATALOG.get_or_init(Mutex::default)
}

fn lock_catalog() -> std::sync::MutexGuard<'static, Catalog> {
    catalog().lock().unwrap_or_else(|error| {
        tracing::error!(%error, "Codex catalog lock poisoned");
        error.into_inner()
    })
}

fn cache_path() -> PathBuf {
    crate::util::data_dir().join("codex_models.json")
}

/// Use the same executable as the agent runner. Load the last successful catalog
/// immediately, then refresh in the background; never block startup on the CLI.
pub(crate) fn configure(binary: PathBuf) {
    {
        let mut state = lock_catalog();
        if state.binary.as_ref() == Some(&binary) {
            return;
        }
        state.models.clear();
        match std::fs::read(cache_path()) {
            Ok(bytes) => match serde_json::from_slice::<Cache>(&bytes) {
                Ok(cache) if cache.binary == binary => {
                    state.models = normalize(cache.models);
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "Invalid cached Codex catalog"),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(%error, "Cannot read cached Codex catalog"),
        }
        state.binary = Some(binary);
        state.refreshing = false;
        state.next_refresh = None;
    }
    refresh_if_due();
}

fn normalize(models: Vec<CatalogModel>) -> Vec<CatalogModel> {
    let mut seen = HashSet::new();
    let mut models: Vec<_> = models
        .into_iter()
        .filter(|model| !model.hidden && !model.model.trim().is_empty())
        .filter(|model| seen.insert(model.model.clone()))
        .collect();
    let default = models
        .iter()
        .position(|model| model.is_default)
        .unwrap_or(0);
    for (index, model) in models.iter_mut().enumerate() {
        model.is_default = index == default;
    }
    models
}

fn refresh_if_due() {
    let mut state = lock_catalog();
    if state.refreshing || state.next_refresh.is_some_and(|next| next > Instant::now()) {
        return;
    }
    let Some(binary) = state.binary.clone() else {
        return;
    };
    state.refreshing = true;
    let path = cache_path();
    if let Err(error) = std::thread::Builder::new()
        .name("codex-model-discovery".into())
        .spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .and_then(|runtime| runtime.block_on(discover(&binary)));
            let mut state = lock_catalog();
            // An older executable's in-flight discovery must not replace a new catalog.
            if state.binary.as_ref() != Some(&binary) {
                return;
            }
            if state.complete_refresh(result) {
                let cache = Cache {
                    binary,
                    models: state.models.clone(),
                };
                if let Err(error) = save_cache(&path, &cache) {
                    tracing::warn!(%error, "Cannot save Codex catalog");
                }
            }
        })
    {
        state.refreshing = false;
        state.next_refresh = Some(Instant::now() + Duration::from_secs(60));
        tracing::warn!(%error, "Cannot start Codex model discovery");
    }
}

pub(crate) fn models() -> Option<Vec<ModelInfo>> {
    refresh_if_due();
    let state = lock_catalog();
    if state.models.is_empty() {
        return None;
    }
    Some(
        state
            .models
            .iter()
            .map(|model| {
                let mut info = ModelInfo::new(
                    AgentType::Codex,
                    &model.model,
                    &model.display_name,
                    &model.model,
                    &model.description,
                    ModelRegistry::fallback_codex_models()
                        .iter()
                        .find(|known| known.id == model.model)
                        .map_or(ModelRegistry::CODEX_CONTEXT_WINDOW, |known| {
                            known.context_window
                        }),
                );
                info.is_default = model.is_default;
                info
            })
            .collect(),
    )
}

async fn discover(binary: &Path) -> io::Result<Vec<CatalogModel>> {
    discover_with_timeout(binary, Duration::from_secs(20)).await
}

async fn discover_with_timeout(binary: &Path, timeout: Duration) -> io::Result<Vec<CatalogModel>> {
    let mut child = tokio::process::Command::new(binary)
        .arg("app-server")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let result = tokio::time::timeout(timeout, query_catalog(&mut child))
        .await
        .unwrap_or_else(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Codex model discovery timed out",
            ))
        });
    // Reap the process on success, protocol errors, and timeout alike.
    child.kill().await?;
    result
}

async fn query_catalog(child: &mut tokio::process::Child) -> io::Result<Vec<CatalogModel>> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("missing stdin"))?;
    let mut stdout = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing stdout"))?,
    );
    let initialize = json!({
        "id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "conduit", "title": "Conduit", "version": env!("CARGO_PKG_VERSION")}
        }
    });
    request(&mut stdin, &mut stdout, initialize).await?;
    stdin
        .write_all(b"{\"method\":\"initialized\",\"params\":{}}\n")
        .await?;
    let mut models = Vec::new();
    let mut cursor = None::<String>;
    let mut cursors = HashSet::new();
    loop {
        let message =
            json!({"id": 2, "method": "model/list", "params": {"limit": 100, "cursor": cursor}});
        let value = request(&mut stdin, &mut stdout, message).await?;
        let page: ModelPage = serde_json::from_value(value)?;
        models.extend(page.data);
        cursor = page.next_cursor;
        match &cursor {
            None => break,
            Some(next) if !cursors.insert(next.clone()) => {
                return Err(io::Error::other("repeated Codex model cursor"))
            }
            Some(_) => {}
        }
    }
    let models = normalize(models);
    if models.is_empty() {
        return Err(io::Error::other("Codex returned no visible models"));
    }
    Ok(models)
}

async fn request(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut BufReader<tokio::process::ChildStdout>,
    message: Value,
) -> io::Result<Value> {
    stdin.write_all(format!("{message}\n").as_bytes()).await?;
    stdin.flush().await?;
    loop {
        let mut line = String::new();
        if stdout.read_line(&mut line).await? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Codex closed model discovery connection",
            ));
        }
        let response: Value = serde_json::from_str(&line)?;
        if response.get("id") != message.get("id") {
            continue;
        }
        if let Some(error) = response.get("error") {
            return Err(io::Error::other(format!("Codex model discovery: {error}")));
        }
        return response
            .get("result")
            .cloned()
            .ok_or_else(|| io::Error::other("Codex response missing result"));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_cli(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temporary CLI directory");
        let path = dir.path().join("codex");
        std::fs::write(&path, format!("#!/usr/bin/env python3\n{body}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path)
    }

    #[tokio::test]
    async fn discovery_handles_pages_and_future_metadata() {
        let (_dir, path) = fake_cli(
            r#"
import json, sys
assert 'app-server' in sys.argv
first = json.loads(input())
assert first['method'] == 'initialize'
assert first['params']['clientInfo']['name'] == 'conduit'
print(json.dumps({'id': first['id'], 'result': {}}), flush=True)
assert json.loads(input())['method'] == 'initialized'
for cursor in [None, 'page-2']:
    request = json.loads(input())
    assert request['method'] == 'model/list'
    assert request['params']['cursor'] == cursor
    model = {'id': 'picker-id', 'model': 'future-model', 'displayName': 'Future Model',
             'isDefault': True, 'supportedReasoningEfforts': [{'reasoningEffort': 'future-effort'}]}
    hidden = dict(model, model='hidden-model', hidden=True)
    print(json.dumps({'method': 'notification', 'params': {}}), flush=True)
    print(json.dumps({'id': request['id'], 'result': {
        'data': [model, hidden], 'nextCursor': 'page-2' if cursor is None else None}}), flush=True)
sys.stdin.read()
"#,
        );
        let models = discover(&path).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model, "future-model");
        assert_eq!(models[0].display_name, "Future Model");
        assert!(models[0].is_default);
    }

    #[tokio::test]
    async fn discovery_reports_rpc_errors() {
        let (_dir, path) = fake_cli(
            r#"
import json, sys
request = json.loads(input())
print(json.dumps({'id': request['id'], 'error': {'code': -1, 'message': 'not signed in'}}), flush=True)
sys.stdin.read()
"#,
        );
        let error = discover(&path).await.unwrap_err();
        assert!(error.to_string().contains("not signed in"));
    }

    #[tokio::test]
    async fn discovery_bounds_wait_and_reaps_process() {
        let (dir, path) = fake_cli(
            r#"
import os, pathlib, sys
pathlib.Path(__file__).with_suffix('.pid').write_text(str(os.getpid()))
sys.stdin.read()
"#,
        );
        let error = discover_with_timeout(&path, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let pid = std::fs::read_to_string(dir.path().join("codex.pid")).unwrap();
        let status = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "discovery child should have been reaped");
    }

    #[test]
    fn normalization_selects_one_default_and_filters_invalid_entries() {
        let models: Vec<CatalogModel> = serde_json::from_value(json!([
            {"model": "", "displayName": "Invalid"},
            {"model": "hidden", "displayName": "Hidden", "hidden": true},
            {"model": "first", "displayName": "First"},
            {"model": "second", "displayName": "Second", "isDefault": true},
            {"model": "third", "displayName": "Third", "isDefault": true}
        ]))
        .unwrap();
        let models = normalize(models);
        assert_eq!(models.len(), 3);
        assert!(!models[0].is_default);
        assert!(models[1].is_default);
        assert!(!models[2].is_default);
        let mut without_default = models;
        without_default
            .iter_mut()
            .for_each(|model| model.is_default = false);
        assert!(normalize(without_default)[0].is_default);
    }

    #[test]
    fn cached_catalog_survives_discovery_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codex_models.json");
        let models = serde_json::from_value(json!([
            {"model": "cached-model", "displayName": "Cached", "isDefault": true}
        ]))
        .unwrap();
        let cache = Cache {
            binary: "custom-codex".into(),
            models,
        };
        save_cache(&path, &cache).unwrap();
        let cached: Cache = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(cached.binary, PathBuf::from("custom-codex"));
        let mut state = Catalog {
            models: cached.models,
            refreshing: true,
            ..Default::default()
        };
        assert!(!state.complete_refresh(Err(io::Error::other("offline"))));
        assert_eq!(state.models[0].model, "cached-model");
        assert!(state.models[0].is_default);
        assert!(!state.refreshing);
        assert!(state.next_refresh.unwrap() > Instant::now());
    }

    #[tokio::test]
    #[ignore = "requires an installed Codex CLI; catalog discovery only, no inference"]
    async fn installed_cli_catalog() {
        let binary = which::which("codex").unwrap();
        let models = discover(&binary).await.unwrap();
        assert!(!models.is_empty());
        assert_eq!(models.iter().filter(|model| model.is_default).count(), 1);
        eprintln!(
            "Discovered models: {:?}",
            models.iter().map(|model| &model.model).collect::<Vec<_>>()
        );
    }
}
