use crate::importer::KsmScore;
use anyhow::{bail, Result};
use async_std::sync::Mutex;
use lazy_static::lazy_static;
use rusqlite::{params, Connection};
use std::{
    collections::HashMap,
    ffi::OsStr,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub type ImportFn = fn(&KsmScore, &Connection, &Path) -> Result<()>;

fn get_score_chart_path(score_path: &Path) -> Result<PathBuf> {
    let mut res = score_path.with_extension("ksh");
    let depth = res.components().count();
    res = res
        .components()
        .enumerate()
        .filter(|(i, _)| *i != depth - 4)
        .map(|(i, c)| {
            if i == depth - 5 {
                Component::Normal(OsStr::new("songs"))
            } else {
                c
            }
        })
        .collect();

    if !res.exists() {
        bail!(
            "File does not exist: \"{}\"",
            res.to_str().unwrap_or_default()
        );
    }

    Ok(res)
}

lazy_static! {
    static ref HASH_CACHE: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

fn hash_file(path: &Path) -> Result<String> {
    let mut cache = HASH_CACHE.try_lock().unwrap();

    let key = path.to_str().unwrap_or_default().to_string();
    if cache.contains_key(&key) {
        println!("Cache hit");
        return Ok(cache.get(&key).unwrap().clone());
    }

    let mut f = std::fs::File::open(path)?;
    let mut hasher = sha1::Sha1::new();
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    hasher.update(buf.as_slice());
    let res = hasher.digest().to_string();
    cache.insert(key, res.clone());
    Ok(res)
}

pub fn version_19(score: &KsmScore, db: &Connection, score_path: &Path) -> Result<()> {
    let chart_path = get_score_chart_path(score_path)?;
    let lwt = std::fs::metadata(&score_path)?.modified()?;
    let lwt = lwt.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let hash = hash_file(&chart_path)?;
    let gauge_type = if score.hard { 1 } else { 0 };
    db.execute(
        "INSERT INTO 
        Scores(score,crit,near,miss,gauge,auto_flags,replay,timestamp,chart_hash,user_name,user_id,local_score,window_perfect,window_good,window_hold,window_miss,window_slam,gauge_type,gauge_opt,mirror,random) 
        VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", params![score.score, score.crit, score.near, score.miss, score.gauge as f32, 0, "", lwt, hash, "", 0, true, 46, 92, 138, 250, 84, gauge_type, 0, false, false]
    )?;
    Ok(())
}
