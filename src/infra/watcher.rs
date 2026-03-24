use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Result};
use tokio::sync::mpsc;
use tokio::time::{sleep, Instant};

pub struct FileWatcher;

impl FileWatcher {
    pub async fn watch<F>(paths: &[PathBuf], callback: F) -> Result<()>
    where
        F: Fn(PathBuf) + Send + Sync + 'static,
    {
        if paths.is_empty() {
            return Err(anyhow!("no bibliography files provided to watcher"));
        }

        let (tx, rx) = mpsc::channel(128);

        #[cfg(feature = "notify")]
        let _watcher = watch_with_notify(paths, tx.clone())?;

        #[cfg(not(feature = "notify"))]
        let _poller = tokio::spawn(poll_for_changes(paths.to_vec(), tx.clone()));

        drop(tx);
        debounce_events(rx, callback).await
    }
}

#[cfg(feature = "notify")]
fn watch_with_notify(
    paths: &[PathBuf],
    tx: mpsc::Sender<PathBuf>,
) -> Result<notify::RecommendedWatcher> {
    use notify::{recommended_watcher, RecursiveMode, Watcher};

    let mut watcher = recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            for path in event.paths {
                let _ = tx.blocking_send(path);
            }
        }
    })?;

    for path in paths {
        watcher.watch(path, RecursiveMode::NonRecursive)?;
    }

    Ok(watcher)
}

#[cfg(not(feature = "notify"))]
async fn poll_for_changes(paths: Vec<PathBuf>, tx: mpsc::Sender<PathBuf>) {
    let mut last_seen = paths
        .iter()
        .map(|path| (path.clone(), file_signature(path)))
        .collect::<HashMap<_, _>>();

    loop {
        for path in &paths {
            let signature = file_signature(path);
            let changed = last_seen.get(path) != Some(&signature);
            if changed {
                last_seen.insert(path.clone(), signature);
                if tx.send(path.clone()).await.is_err() {
                    return;
                }
            }
        }

        sleep(Duration::from_millis(150)).await;
    }
}

#[cfg(not(feature = "notify"))]
fn file_signature(path: &PathBuf) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

async fn debounce_events<F>(mut rx: mpsc::Receiver<PathBuf>, callback: F) -> Result<()>
where
    F: Fn(PathBuf) + Send + Sync + 'static,
{
    let callback = Arc::new(callback);
    let mut pending = HashMap::<PathBuf, Instant>::new();
    let debounce = Duration::from_millis(200);

    loop {
        if pending.is_empty() {
            match rx.recv().await {
                Some(path) => {
                    pending.insert(path, Instant::now() + debounce);
                }
                None => return Ok(()),
            }
            continue;
        }

        let next_deadline = pending
            .values()
            .min()
            .copied()
            .expect("pending map is not empty");
        let sleeper = sleep(next_deadline.saturating_duration_since(Instant::now()));
        tokio::pin!(sleeper);

        tokio::select! {
            maybe_path = rx.recv() => {
                match maybe_path {
                    Some(path) => {
                        pending.insert(path, Instant::now() + debounce);
                    }
                    None => flush_pending(&mut pending, &callback),
                }
            }
            _ = &mut sleeper => {
                flush_ready(&mut pending, &callback);
                if pending.is_empty() && rx.is_closed() {
                    return Ok(());
                }
            }
        }
    }
}

fn flush_pending<F>(pending: &mut HashMap<PathBuf, Instant>, callback: &Arc<F>)
where
    F: Fn(PathBuf) + Send + Sync + 'static,
{
    let ready = pending.drain().map(|(path, _)| path).collect::<Vec<_>>();
    for path in ready {
        callback(path);
    }
}

fn flush_ready<F>(pending: &mut HashMap<PathBuf, Instant>, callback: &Arc<F>)
where
    F: Fn(PathBuf) + Send + Sync + 'static,
{
    let now = Instant::now();
    let ready = pending
        .iter()
        .filter_map(|(path, deadline)| {
            if *deadline <= now {
                Some(path.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    for path in ready {
        pending.remove(&path);
        callback(path);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn debounce_coalesces_rapid_changes_for_same_path() {
        let (tx, rx) = mpsc::channel(16);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let collected = Arc::clone(&seen);

        let task = tokio::spawn(async move {
            debounce_events(rx, move |path| collected.lock().unwrap().push(path))
                .await
                .unwrap();
        });

        let path = PathBuf::from("library.bib");
        tx.send(path.clone()).await.unwrap();
        tx.send(path.clone()).await.unwrap();
        tx.send(path.clone()).await.unwrap();
        drop(tx);

        task.await.unwrap();

        assert_eq!(seen.lock().unwrap().as_slice(), &[path]);
    }

    #[tokio::test]
    async fn debounce_delivers_distinct_paths() {
        let (tx, rx) = mpsc::channel(16);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let collected = Arc::clone(&seen);

        let task = tokio::spawn(async move {
            debounce_events(rx, move |path| collected.lock().unwrap().push(path))
                .await
                .unwrap();
        });

        let alpha = PathBuf::from("alpha.bib");
        let beta = PathBuf::from("beta.bib");
        tx.send(alpha.clone()).await.unwrap();
        tx.send(beta.clone()).await.unwrap();
        drop(tx);

        task.await.unwrap();

        let mut seen = seen.lock().unwrap().clone();
        seen.sort();
        assert_eq!(seen, vec![alpha, beta]);
    }

    #[tokio::test]
    async fn polling_backend_detects_external_file_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("library.bib");
        std::fs::write(&path, "@article{alpha,\n  title = {Alpha}\n}\n").unwrap();

        let seen = Arc::new(Mutex::new(Vec::new()));
        let collected = Arc::clone(&seen);
        let watch_path = path.clone();

        let task = tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(2), FileWatcher::watch(&[watch_path], move |changed| {
                collected.lock().unwrap().push(changed);
            }))
            .await;
        });

        sleep(Duration::from_millis(250)).await;
        std::fs::write(&path, "@article{alpha,\n  title = {Updated}\n}\n").unwrap();
        sleep(Duration::from_millis(500)).await;
        task.abort();

        assert!(seen.lock().unwrap().iter().any(|changed| changed == &path));
    }
}
