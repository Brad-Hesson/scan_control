use std::{
    fmt::Debug,
    fs::DirEntry,
    path::{Path, PathBuf},
};

use eyre::{bail, ContextCompat, Ok, Result};
use sxmfile::SXM;
use tracing::error;

#[derive(Debug)]
pub struct FileTree {
    top: DirItem,
}
impl FileTree {
    pub fn load_path(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            top: DirItem::load_dir(path)?,
        })
    }
}

pub enum DirItem {
    Folder { name: String, items: Vec<DirItem> },
    File { src: SXM },
}
impl DirItem {
    fn load_entry(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let file_type = std::fs::metadata(&path)?.file_type();
        if file_type.is_dir() {
            Ok(Some(Self::load_dir(path)?))
        } else if file_type.is_file() {
            Self::load_file(path)
        } else {
            error!("Encountered a symlink");
            Ok(None)
        }
    }
    fn load_dir(path: impl AsRef<Path>) -> Result<Self> {
        let mut items = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            if let Some(item) = Self::load_entry(entry.path())? {
                items.push(item);
            }
        }
        Ok(Self::Folder {
            items,
            name: path
                .as_ref()
                .file_name()
                .unwrap()
                .to_str()
                .context("folder name was not valid utf-8")?
                .to_string(),
        })
    }
    fn load_file(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let file_extension = path
            .as_ref()
            .extension()
            .unwrap_or_default()
            .to_str()
            .context("file extension not valid utf-8")?;
        if file_extension != "sxm" {
            return Ok(None);
        }
        let src = SXM::parse_file(path)?;
        Ok(Some(Self::File { src }))
    }
}
impl Debug for DirItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Folder { items, name } => f
                .debug_struct("Folder")
                .field("name", name)
                .field("items", items)
                .finish(),
            Self::File { src } => f
                .debug_struct("File")
                .field("name", &src.get_name().unwrap())
                .finish(),
        }
    }
}
