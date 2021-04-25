use crate::Summary;
use anyhow::{ensure, Result};
use iced_futures::futures;
use rusqlite::Connection;
use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use walkdir::DirEntry;

pub fn import(ksm_path: &PathBuf, db_path: &PathBuf) -> Result<iced::Subscription<Progress>> {
    ensure!(ksm_path.exists(), "KSM path invalid: {:?}", ksm_path);
    ensure!(db_path.exists(), "maps.db path invalid: {:?}", db_path);

    Ok(iced::Subscription::from_recipe(Importer {
        db_path: db_path.clone(),
        ksm_path: ksm_path.clone(),
    }))
}

pub struct KsmScore {
    pub score: u32,
    pub crit: u32,
    pub near: u32,
    pub miss: u32,
    pub gauge: f64,
    pub badge: u32,
    pub hard: bool,
}

impl FromStr for KsmScore {
    type Err = anyhow::Error;

    fn from_str(score_line: &str) -> Result<Self, Self::Err> {
        ensure!(
            score_line.starts_with("hard,normal,normal,on,on,on")
                || score_line.starts_with("normal,normal,normal,on,on,on"),
            "Unsupported score entry"
        );

        let (settings, stats): (Vec<&str>, Vec<&str>) = {
            let mut parts = score_line.split("=");
            (
                parts.next().unwrap().split(",").collect(),
                parts.next().unwrap().split(",").collect(),
            )
        };
        let hard = settings[0] == "hard";
        let score: u32 = stats[0].parse()?;
        let gauge: f64 = stats[3].parse::<f64>()? / 100.0;
        let badge: u32 = stats[1].parse()?;
        let miss = if badge > 1 { 0 } else { 1 };
        Ok(Self {
            score,
            crit: 0,
            near: 0,
            miss,
            gauge,
            badge,
            hard,
        })
    }
}

fn enumerate_ksm_score_files(ksm_path: &PathBuf) -> Result<Vec<DirEntry>> {
    let mut score_paths = ksm_path.clone();
    score_paths.push("score");
    ensure!(
        score_paths.exists(),
        "Path does not exist: {:?}",
        score_paths.to_str(),
    );

    let dirs = walkdir::WalkDir::new(score_paths);
    Ok(dirs
        .into_iter()
        .filter(|p| p.is_ok())
        .filter(|p| p.as_ref().unwrap().path().extension().is_some())
        .filter(|p| {
            p.as_ref()
                .unwrap()
                .path()
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .to_ascii_lowercase()
                .eq(&"ksc".to_string())
        })
        .map(|d| d.unwrap().clone())
        .collect())
}

pub struct Importer {
    db_path: PathBuf,
    ksm_path: PathBuf,
}

async fn run_importer(state: State) -> Option<(Progress, State)> {
    match state {
        State::Ready { ksm, db } => {
            let db_conn = Connection::open(db.as_path());
            let score_files = enumerate_ksm_score_files(&ksm);

            match (db_conn, score_files) {
                (Ok(db), Ok(ksm)) => Some((
                    Progress::Started,
                    State::Importing {
                        db_version: db
                            .query_row("SELECT version FROM `Database`", [], |r| r.get(0))
                            .unwrap_or_default(),
                        connection: db,
                        summary: Summary {
                            scores_found: ksm.len() as u32,
                            ..Default::default()
                        },
                        score_files: ksm,
                    },
                )),
                (Ok(_), Err(e)) => Some((Progress::Errored(format!("{:?}", e)), State::Finished)),
                (Err(e), Ok(_)) => Some((Progress::Errored(format!("{:?}", e)), State::Finished)),
                (Err(db_err), Err(ksm_err)) => Some((
                    Progress::Errored(format!(
                        "DB Error: '{:?}', KSM Path error: '{:?}'",
                        db_err, ksm_err
                    )),
                    State::Finished,
                )),
            }
        }
        State::Importing {
            mut score_files,
            mut summary,
            connection,
            db_version,
        } => {
            if score_files.is_empty() {
                return Some((Progress::Finished(summary), State::Finished));
            }

            let insert_func: Option<fn(&KsmScore, &Connection, &PathBuf) -> Result<()>> =
                match db_version {
                    19 => Some(crate::importer_funcs::version_19),
                    _ => None,
                };
            if insert_func.is_none() {
                return Some((
                    Progress::Errored(format!("Unsupported DB version: {}", db_version)),
                    State::Finished,
                ));
            }

            let insert_func = insert_func.unwrap();

            let current_file_path = score_files.pop().unwrap().path().to_path_buf();

            match std::fs::File::open(&current_file_path) {
                Ok(current_file) => {
                    let scores_imported = &mut summary.scores_imported;
                    let fail_messages = Rc::new(RefCell::new(&mut summary.fail_messages));
                    BufReader::new(current_file)
                        .lines()
                        .filter(|l| l.is_ok())
                        .map(|l| KsmScore::from_str(&l.unwrap()))
                        .filter(|s| match s {
                            Ok(_) => true,
                            Err(e) => {
                                fail_messages.borrow_mut().push(format!(
                                    "Score parse failed in \"{}\": {:?}",
                                    current_file_path.to_str().unwrap_or_default(),
                                    e
                                ));
                                false
                            }
                        })
                        .map(|s| s.unwrap())
                        .filter(|s| match insert_func(&s, &connection, &current_file_path) {
                            Ok(_) => true,
                            Err(e) => {
                                fail_messages
                                    .borrow_mut()
                                    .push(format!("Score insert failed: {:?}", e));
                                false
                            }
                        })
                        .for_each(|_| *scores_imported += 1);
                }
                Err(e) => summary.fail_messages.push(format!(
                    "Failed to open \"{}\": {:?}",
                    current_file_path.to_str().unwrap_or_default(),
                    e
                )),
            }

            let progress = 1.0 - (score_files.len() as f32 / summary.scores_found as f32);
            Some((
                Progress::Advanced(progress),
                State::Importing {
                    db_version,
                    score_files,
                    summary,
                    connection,
                },
            ))
        }
        State::Finished => None,
    }
}

impl<H, I> iced_native::subscription::Recipe<H, I> for Importer
where
    H: std::hash::Hasher,
{
    type Output = Progress;

    fn hash(&self, state: &mut H) {
        use std::hash::Hash;

        std::any::TypeId::of::<Self>().hash(state);
        self.db_path.hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: iced_futures::BoxStream<I>,
    ) -> iced_futures::BoxStream<Self::Output> {
        Box::pin(futures::stream::unfold(
            State::Ready {
                ksm: self.ksm_path,
                db: self.db_path,
            },
            run_importer,
        ))
    }
}

#[derive(Debug)]
enum State {
    Ready {
        ksm: PathBuf,
        db: PathBuf,
    },
    Importing {
        db_version: u32,
        score_files: Vec<DirEntry>,
        summary: Summary,
        connection: Connection,
    },
    Finished,
}

#[derive(Debug, Clone)]
pub enum Progress {
    Started,
    Advanced(f32),
    Finished(Summary),
    Errored(String),
}
