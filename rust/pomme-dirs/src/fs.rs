//! The file system Pomme reads and writes through: `std::fs` on desktop, and
//! in the browser, which has none, a tree of files in memory. The page fills
//! that tree from its stored files before the game starts ([`preload`]),
//! hears of each change to keep its store current ([`set_persist`]), and may
//! be asked for a missing file to load later ([`set_fetch`]).
//!
//! Besides `std::fs`'s functions, [`exists`], [`is_dir`] and [`is_file`]
//! stand in for `Path`'s methods of those names, which always ask the real
//! file system.

#[cfg(not(target_arch = "wasm32"))]
pub use std::fs::*;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
pub fn exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn is_dir(path: impl AsRef<Path>) -> bool {
    path.as_ref().is_dir()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn is_file(path: impl AsRef<Path>) -> bool {
    path.as_ref().is_file()
}

#[cfg(target_arch = "wasm32")]
pub use memory::*;

#[cfg(target_arch = "wasm32")]
mod memory {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
    use std::path::{Component, Path, PathBuf};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

    use web_time::SystemTime;

    #[derive(Clone)]
    enum Node {
        File {
            bytes: Arc<Vec<u8>>,
            modified: SystemTime,
        },
        Dir,
    }

    /// A change for the page's store to make.
    pub enum Change<'a> {
        Write(&'a Path, &'a [u8]),
        /// `path` and everything under it.
        Remove(&'a Path),
    }

    static TREE: Mutex<BTreeMap<PathBuf, Node>> = Mutex::new(BTreeMap::new());
    static PERSIST: OnceLock<fn(Change<'_>)> = OnceLock::new();
    static FETCH: OnceLock<fn(&Path) -> bool> = OnceLock::new();

    fn tree() -> MutexGuard<'static, BTreeMap<PathBuf, Node>> {
        TREE.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Tells the page's store of every change from now on.
    pub fn set_persist(persist: fn(Change<'_>)) {
        let _ = PERSIST.set(persist);
    }

    /// Asked for each file read that's missing; true when it will load the
    /// file in the background, which the read then reports as busy rather
    /// than missing.
    pub fn set_fetch(fetch: fn(&Path) -> bool) {
        let _ = FETCH.set(fetch);
    }

    fn persist(change: Change<'_>) {
        if let Some(persist) = PERSIST.get() {
            persist(change);
        }
    }

    /// `path` absolute, without `.` or `..`.
    fn normal(path: &Path) -> PathBuf {
        let mut out = PathBuf::from("/");
        for component in path.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::ParentDir => {
                    out.pop();
                }
                Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
            }
        }
        out
    }

    fn not_found(path: &Path) -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, format!("{}: no such file", path.display()))
    }

    fn add_parents(tree: &mut BTreeMap<PathBuf, Node>, path: &Path) {
        let mut parent = path.parent();
        while let Some(dir) = parent {
            tree.entry(dir.to_path_buf()).or_insert(Node::Dir);
            parent = dir.parent();
        }
    }

    /// Adds a stored file to the tree, without telling the store again.
    pub fn preload(path: impl AsRef<Path>, bytes: Vec<u8>) {
        let path = normal(path.as_ref());
        let mut tree = tree();
        add_parents(&mut tree, &path);
        tree.insert(
            path,
            Node::File {
                bytes: Arc::new(bytes),
                modified: SystemTime::now(),
            },
        );
    }

    pub fn exists(path: impl AsRef<Path>) -> bool {
        tree().contains_key(&normal(path.as_ref()))
    }

    pub fn is_dir(path: impl AsRef<Path>) -> bool {
        matches!(tree().get(&normal(path.as_ref())), Some(Node::Dir))
    }

    pub fn is_file(path: impl AsRef<Path>) -> bool {
        matches!(tree().get(&normal(path.as_ref())), Some(Node::File { .. }))
    }

    fn read_shared(path: &Path) -> io::Result<Arc<Vec<u8>>> {
        let normal = normal(path);
        match tree().get(&normal) {
            Some(Node::File { bytes, .. }) => return Ok(Arc::clone(bytes)),
            Some(Node::Dir) => {
                return Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    format!("{}: is a directory", path.display()),
                ));
            }
            None => {}
        }
        if FETCH.get().is_some_and(|fetch| fetch(&normal)) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("{}: still loading", path.display()),
            ));
        }
        Err(not_found(path))
    }

    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        read_shared(path.as_ref()).map(|bytes| bytes.to_vec())
    }

    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
        let path = normal(path.as_ref());
        let bytes = contents.as_ref();
        {
            let mut tree = tree();
            if matches!(tree.get(&path), Some(Node::Dir)) {
                return Err(io::Error::new(io::ErrorKind::IsADirectory, "is a directory"));
            }
            add_parents(&mut tree, &path);
            tree.insert(
                path.clone(),
                Node::File {
                    bytes: Arc::new(bytes.to_vec()),
                    modified: SystemTime::now(),
                },
            );
        }
        persist(Change::Write(&path, bytes));
        Ok(())
    }

    pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        let bytes = read_shared(from.as_ref())?;
        write(to, &**bytes)?;
        Ok(bytes.len() as u64)
    }

    pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        let path = normal(path.as_ref());
        let mut tree = tree();
        if matches!(tree.get(&path), Some(Node::File { .. })) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a file is in the way"));
        }
        add_parents(&mut tree, &path);
        tree.insert(path, Node::Dir);
        Ok(())
    }

    pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
        if exists(path.as_ref()) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{}: already exists", path.as_ref().display()),
            ));
        }
        create_dir_all(path)
    }

    fn remove_under(tree: &mut BTreeMap<PathBuf, Node>, path: &Path) {
        let doomed: Vec<PathBuf> = tree
            .range(path.to_path_buf()..)
            .take_while(|(p, _)| p.starts_with(path))
            .map(|(p, _)| p.clone())
            .collect();
        for p in doomed {
            tree.remove(&p);
        }
    }

    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        let path = normal(path.as_ref());
        match tree().remove(&path) {
            Some(Node::File { .. }) => {}
            Some(Node::Dir) => return Err(io::Error::new(io::ErrorKind::IsADirectory, "is a directory")),
            None => return Err(not_found(&path)),
        }
        persist(Change::Remove(&path));
        Ok(())
    }

    pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        let path = normal(path.as_ref());
        {
            let mut tree = tree();
            if !tree.contains_key(&path) {
                return Err(not_found(&path));
            }
            remove_under(&mut tree, &path);
        }
        persist(Change::Remove(&path));
        Ok(())
    }

    pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
        remove_dir_all(path)
    }

    pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        let (from, to) = (normal(from.as_ref()), normal(to.as_ref()));
        let moved: Vec<(PathBuf, Node)> = {
            let mut tree = tree();
            if !tree.contains_key(&from) {
                return Err(not_found(&from));
            }
            remove_under(&mut tree, &to);
            let moved: Vec<(PathBuf, Node)> = tree
                .range(from.clone()..)
                .take_while(|(p, _)| p.starts_with(&from))
                .map(|(p, n)| (to.join(p.strip_prefix(&from).unwrap()), n.clone()))
                .collect();
            remove_under(&mut tree, &from);
            add_parents(&mut tree, &to);
            for (p, n) in &moved {
                tree.insert(p.clone(), n.clone());
            }
            moved
        };
        persist(Change::Remove(&from));
        persist(Change::Remove(&to));
        for (p, n) in &moved {
            if let Node::File { bytes, .. } = n {
                persist(Change::Write(p, bytes));
            }
        }
        Ok(())
    }

    pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        let path = normal(path.as_ref());
        if tree().contains_key(&path) { Ok(path) } else { Err(not_found(&path)) }
    }

    pub fn read_link(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: not a link", path.as_ref().display()),
        ))
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FileType {
        dir: bool,
    }

    impl FileType {
        pub fn is_dir(&self) -> bool {
            self.dir
        }

        pub fn is_file(&self) -> bool {
            !self.dir
        }

        pub fn is_symlink(&self) -> bool {
            false
        }
    }

    #[derive(Clone, Debug)]
    pub struct Metadata {
        len: u64,
        file_type: FileType,
        modified: SystemTime,
    }

    impl Metadata {
        pub fn len(&self) -> u64 {
            self.len
        }

        pub fn is_dir(&self) -> bool {
            self.file_type.dir
        }

        pub fn is_file(&self) -> bool {
            !self.file_type.dir
        }

        pub fn is_symlink(&self) -> bool {
            false
        }

        pub fn file_type(&self) -> FileType {
            self.file_type
        }

        pub fn modified(&self) -> io::Result<SystemTime> {
            Ok(self.modified)
        }
    }

    fn metadata_of(node: &Node) -> Metadata {
        match node {
            Node::File { bytes, modified } => Metadata {
                len: bytes.len() as u64,
                file_type: FileType { dir: false },
                modified: *modified,
            },
            Node::Dir => Metadata {
                len: 0,
                file_type: FileType { dir: true },
                modified: SystemTime::UNIX_EPOCH,
            },
        }
    }

    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        let path = normal(path.as_ref());
        tree().get(&path).map(metadata_of).ok_or_else(|| not_found(&path))
    }

    pub fn symlink_metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        metadata(path)
    }

    pub struct DirEntry {
        path: PathBuf,
        metadata: Metadata,
    }

    impl DirEntry {
        pub fn path(&self) -> PathBuf {
            self.path.clone()
        }

        pub fn file_name(&self) -> OsString {
            self.path.file_name().unwrap_or_default().to_owned()
        }

        pub fn file_type(&self) -> io::Result<FileType> {
            Ok(self.metadata.file_type)
        }

        pub fn metadata(&self) -> io::Result<Metadata> {
            Ok(self.metadata.clone())
        }
    }

    pub struct ReadDir(std::vec::IntoIter<DirEntry>);

    impl Iterator for ReadDir {
        type Item = io::Result<DirEntry>;

        fn next(&mut self) -> Option<Self::Item> {
            self.0.next().map(Ok)
        }
    }

    pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
        let dir = normal(path.as_ref());
        let tree = tree();
        match tree.get(&dir) {
            Some(Node::Dir) => {}
            Some(Node::File { .. }) => {
                return Err(io::Error::new(io::ErrorKind::NotADirectory, "not a directory"));
            }
            None => return Err(not_found(&dir)),
        }
        let entries: Vec<DirEntry> = tree
            .range(dir.clone()..)
            .skip(1)
            .take_while(|(p, _)| p.starts_with(&dir))
            .filter(|(p, _)| p.parent() == Some(dir.as_path()))
            .map(|(p, n)| DirEntry {
                path: p.clone(),
                metadata: metadata_of(n),
            })
            .collect();
        Ok(ReadDir(entries.into_iter()))
    }

    /// An open file: read from a snapshot of its bytes, or written into a
    /// buffer that replaces it on each flush and when dropped.
    pub struct File {
        path: PathBuf,
        data: Cursor<Vec<u8>>,
        writable: bool,
        dirty: bool,
    }

    impl File {
        pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
            OpenOptions::new().read(true).open(path)
        }

        pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
            OpenOptions::new().write(true).create(true).truncate(true).open(path)
        }

        pub fn metadata(&self) -> io::Result<Metadata> {
            Ok(Metadata {
                len: self.data.get_ref().len() as u64,
                file_type: FileType { dir: false },
                modified: SystemTime::now(),
            })
        }

        pub fn sync_all(&mut self) -> io::Result<()> {
            self.flush()
        }

        pub fn set_len(&mut self, size: u64) -> io::Result<()> {
            self.data.get_mut().resize(size as usize, 0);
            self.dirty = true;
            Ok(())
        }
    }

    impl Read for File {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.data.read(buf)
        }
    }

    impl Seek for File {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.data.seek(pos)
        }
    }

    impl Write for File {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if !self.writable {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "opened for reading"));
            }
            self.dirty = true;
            self.data.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.dirty {
                self.dirty = false;
                write(&self.path, self.data.get_ref())?;
            }
            Ok(())
        }
    }

    impl Drop for File {
        fn drop(&mut self) {
            let _ = self.flush();
        }
    }

    #[derive(Clone, Default)]
    pub struct OpenOptions {
        read: bool,
        write: bool,
        append: bool,
        create: bool,
        truncate: bool,
    }

    impl OpenOptions {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn read(&mut self, read: bool) -> &mut Self {
            self.read = read;
            self
        }

        pub fn write(&mut self, write: bool) -> &mut Self {
            self.write = write;
            self
        }

        pub fn append(&mut self, append: bool) -> &mut Self {
            self.append = append;
            self
        }

        pub fn create(&mut self, create: bool) -> &mut Self {
            self.create = create;
            self
        }

        pub fn create_new(&mut self, create: bool) -> &mut Self {
            self.create = create;
            self
        }

        pub fn truncate(&mut self, truncate: bool) -> &mut Self {
            self.truncate = truncate;
            self
        }

        pub fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
            let path = normal(path.as_ref());
            let existing = match read_shared(&path) {
                Ok(bytes) => Some(bytes.to_vec()),
                Err(e) if e.kind() == io::ErrorKind::NotFound && self.create => None,
                Err(e) => return Err(e),
            };
            let writable = self.write || self.append;
            let bytes = if self.truncate { Vec::new() } else { existing.clone().unwrap_or_default() };
            let mut data = Cursor::new(bytes);
            if self.append {
                data.seek(SeekFrom::End(0))?;
            }
            Ok(File {
                path,
                data,
                writable,
                dirty: writable && existing.is_none(),
            })
        }
    }
}
