use std::{
    env::temp_dir,
    error::Error,
    path::{Path, PathBuf},
};

use glam::DAffine2;
use redb::{backends::InMemoryBackend, Database, WriteTransaction};
use uuid::Uuid;

use crate::scan_view::ImageEncoder;

pub struct ProjectDb {
    path: PathBuf,
    db: Database,
    is_temp: bool,
}
impl ProjectDb {
    pub fn new_temp() -> Result<ProjectDb, redb::DatabaseError> {
        let mut path = temp_dir();
        let name = format!("temp_db_{}.scp", Uuid::new_v4());
        path.push(name);
        let db = Database::builder().create(&path)?;
        Ok(Self {
            db,
            path,
            is_temp: true,
        })
    }
    pub fn is_temp(&self) -> bool {
        self.is_temp
    }
    pub fn save_as(&mut self, path: PathBuf) -> Result<(), Box<dyn Error>> {
        let tmp_db = Database::builder().create_with_backend(InMemoryBackend::new())?;
        let mut old_db = std::mem::replace(&mut self.db, tmp_db);
        old_db.compact()?;
        drop(old_db);
        std::fs::copy(&self.path, &path)?;
        *self = Self::open(path)?;
        Ok(())
    }
    pub fn open(path: PathBuf) -> Result<Self, Box<dyn Error>> {
        let new_db = Database::open(path.clone())?;
        Ok(Self {
            db: new_db,
            path: path,
            is_temp: false,
        })
    }
    pub fn current_path(&self) -> &Path {
        &self.path
    }
    pub fn db(&self) -> &Database {
        &self.db
    }
    pub fn update_from(&self, val: &impl Persistant) -> Result<(), Box<dyn Error>> {
        let txn = self.db.begin_write()?;
        val.db_update(&txn)?;
        Ok(())
    }
}

pub trait Persistant: Sized {
    fn db_update<'t>(&self, txn: &'t WriteTransaction) -> Result<(), Box<dyn Error>>;
    fn db_remove<'t>(id: Uuid, txn: &'t WriteTransaction) -> Result<(), Box<dyn Error>>;
    fn db_insert<'t>(&self, txn: &'t WriteTransaction) -> Result<(), Box<dyn Error>>;
    fn db_read<'t>(
        id: Uuid,
        txn: &'t WriteTransaction,
        encoder: &ImageEncoder,
    ) -> Result<Self, Box<dyn Error>>;
}
