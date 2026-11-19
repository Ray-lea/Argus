//! Memory-backed day files for the corruption corpus. Other test directories
//! use the real filesystem, including the on-disk recovery regression tests.
use super::*;
use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;

type Bytes = Rc<RefCell<Vec<u8>>>;
struct Day {
    dir: PathBuf,
    number: i64,
    files: HashMap<PathBuf, Bytes>,
}
thread_local! { static STORE: RefCell<Option<Day>> = const { RefCell::new(None) }; }

pub(super) fn install(dir: &Path, number: i64) {
    STORE.set(Some(Day {
        dir: dir.into(),
        number,
        files: HashMap::new(),
    }));
}

pub(super) fn day(dir: &Path) -> Option<i64> {
    STORE.with_borrow(|s| Some(s.as_ref().filter(|s| s.dir == dir)?.number))
}

pub(super) enum File {
    Disk(fs::File),
    Memory { bytes: Bytes, pos: u64 },
}
impl From<fs::File> for File {
    fn from(f: fs::File) -> Self {
        Self::Disk(f)
    }
}
pub(super) fn open(path: &Path, create: bool) -> Option<io::Result<File>> {
    STORE.with_borrow_mut(|s| {
        let s = s.as_mut().filter(|s| path.parent() == Some(&s.dir))?;
        let bytes = if create {
            Some(&*s.files.entry(path.into()).or_default())
        } else {
            s.files.get(path)
        };
        Some(
            bytes
                .cloned()
                .map(|bytes| File::Memory { bytes, pos: 0 })
                .ok_or_else(|| io::ErrorKind::NotFound.into()),
        )
    })
}
impl File {
    pub(super) fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        open(path, false).unwrap_or_else(|| fs::File::open(path).map(Self::Disk))
    }
    pub(super) fn metadata(&self) -> io::Result<Metadata> {
        match self {
            Self::Disk(f) => f.metadata().map(Metadata::Disk),
            Self::Memory { bytes, .. } => Ok(Metadata::Memory(bytes.borrow().len() as u64)),
        }
    }
    pub(super) fn set_len(&self, len: u64) -> io::Result<()> {
        match self {
            Self::Disk(f) => f.set_len(len),
            Self::Memory { bytes, .. } => {
                bytes.borrow_mut().resize(len as usize, 0);
                Ok(())
            }
        }
    }
    fn with_cursor<T>(
        &mut self,
        run: impl FnOnce(&mut Cursor<&mut Vec<u8>>) -> io::Result<T>,
    ) -> io::Result<T> {
        let Self::Memory { bytes, pos } = self else {
            unreachable!()
        };
        let mut bytes = bytes.borrow_mut();
        let mut c = Cursor::new(&mut *bytes);
        c.set_position(*pos);
        let result = run(&mut c);
        *pos = c.position();
        result
    }
}
impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Disk(f) => f.read(buf),
            _ => self.with_cursor(|c| c.read(buf)),
        }
    }
}
impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Disk(f) => f.write(buf),
            _ => self.with_cursor(|c| c.write(buf)),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Disk(f) => f.flush(),
            _ => Ok(()),
        }
    }
}
impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            Self::Disk(f) => f.seek(pos),
            _ => self.with_cursor(|c| c.seek(pos)),
        }
    }
}
pub(super) enum Metadata {
    Disk(fs::Metadata),
    Memory(u64),
}
impl Metadata {
    pub(super) fn len(&self) -> u64 {
        match self {
            Self::Disk(m) => m.len(),
            Self::Memory(n) => *n,
        }
    }
    pub(super) fn modified(&self) -> io::Result<SystemTime> {
        match self {
            Self::Disk(m) => m.modified(),
            Self::Memory(_) => Ok(UNIX_EPOCH),
        }
    }
    pub(super) fn created(&self) -> io::Result<SystemTime> {
        match self {
            Self::Disk(m) => m.created(),
            Self::Memory(_) => Ok(UNIX_EPOCH),
        }
    }
}
pub(super) fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
    let path = path.as_ref();
    open(path, false).map_or_else(|| fs::metadata(path).map(Metadata::Disk), |f| f?.metadata())
}
