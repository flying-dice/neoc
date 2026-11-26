//! The `Files` use case against the recording port: what reaches the port, and what never does.

use crate::{files::Files, testing::RecordingFiles};
use std::sync::Arc;
use vos_domain::{FileErrorKind, Location};

fn files() -> (Files, Arc<RecordingFiles>) {
    let port = Arc::new(RecordingFiles::default());
    (Files::new(port.clone()), port)
}

const DOCS: Location = Location::Documents;

#[tokio::test]
async fn valid_calls_reach_the_port_with_tidied_paths() {
    let (f, port) = files();
    f.read(DOCS, "./notes//a.txt").await.unwrap();
    f.write(Location::Cache, "b.bin", vec![1]).await.unwrap();
    f.list(DOCS, "").await.unwrap();
    f.info(DOCS, "notes/").await.unwrap();
    f.create_dir(Location::Temp, "x/y").await.unwrap();
    f.remove(DOCS, "a", false).await.unwrap();
    f.remove(DOCS, "a", true).await.unwrap();
    f.move_to(DOCS, "a", "b/a").await.unwrap();
    f.move_to(DOCS, "a", "a").await.unwrap();
    f.copy(DOCS, "a/x", "a/y").await.unwrap();
    assert_eq!(
        port.take(),
        [
            "read documents notes/a.txt",
            "write cache b.bin",
            "list documents ",
            "info documents notes",
            "create_dir temp x/y",
            "remove documents a",
            "remove_all documents a",
            "move documents a b/a",
            "move documents a a",
            "copy documents a/x a/y",
        ]
    );
}

#[tokio::test]
async fn an_escaping_path_never_reaches_the_port() {
    let (f, port) = files();
    let long = "a".repeat(300);
    for bad in ["/etc/passwd", "../x", "a/../../x", "a\0b", "C:/x", "a\\b", long.as_str()] {
        let kinds = [
            f.read(DOCS, bad).await.unwrap_err().kind,
            f.write(DOCS, bad, vec![]).await.unwrap_err().kind,
            f.list(DOCS, bad).await.unwrap_err().kind,
            f.info(DOCS, bad).await.unwrap_err().kind,
            f.create_dir(DOCS, bad).await.unwrap_err().kind,
            f.remove(DOCS, bad, true).await.unwrap_err().kind,
            f.move_to(DOCS, "ok", bad).await.unwrap_err().kind,
            f.move_to(DOCS, bad, "ok").await.unwrap_err().kind,
            f.copy(DOCS, "ok", bad).await.unwrap_err().kind,
            f.copy(DOCS, bad, "ok").await.unwrap_err().kind,
        ];
        assert!(kinds.iter().all(|k| *k == FileErrorKind::InvalidInput), "{bad:?}: {kinds:?}");
    }
    assert!(port.take().is_empty());
}

#[tokio::test]
async fn the_location_itself_cannot_be_written_removed_moved_or_copied() {
    let (f, port) = files();
    for root in ["", ".", "./"] {
        assert_eq!(f.write(DOCS, root, vec![]).await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(f.remove(DOCS, root, true).await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(f.move_to(DOCS, root, "a").await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(f.move_to(DOCS, "a", root).await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(f.copy(DOCS, root, "a").await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(f.copy(DOCS, "a", root).await.unwrap_err().kind, FileErrorKind::InvalidInput);
    }
    assert!(port.take().is_empty());
    // reading, listing and creating the location are fine
    f.list(DOCS, "").await.unwrap();
    f.info(DOCS, ".").await.unwrap();
    f.create_dir(DOCS, "").await.unwrap();
    assert_eq!(port.take().len(), 3);
}

#[tokio::test]
async fn a_tree_cannot_be_moved_or_copied_into_itself() {
    let (f, port) = files();
    assert_eq!(f.move_to(DOCS, "a", "a/b").await.unwrap_err().kind, FileErrorKind::InvalidInput);
    assert_eq!(f.copy(DOCS, "a", "a/b").await.unwrap_err().kind, FileErrorKind::InvalidInput);
    assert_eq!(f.copy(DOCS, "a", "a").await.unwrap_err().kind, FileErrorKind::InvalidInput);
    assert!(port.take().is_empty());
    // a sibling whose name starts the same is not inside
    f.move_to(DOCS, "a", "ab").await.unwrap();
    f.copy(DOCS, "a", "ab").await.unwrap();
    assert_eq!(port.take(), ["move documents a ab", "copy documents a ab"]);
}

#[tokio::test]
async fn a_read_only_handle_reads_and_refuses_every_write() {
    let (f, port) = files();
    let ro = f.read_only();
    ro.read(DOCS, "a").await.unwrap();
    ro.list(DOCS, "").await.unwrap();
    ro.info(DOCS, "a").await.unwrap();
    let refused = [
        ro.write(DOCS, "a", vec![]).await.unwrap_err(),
        ro.create_dir(DOCS, "d").await.unwrap_err(),
        ro.remove(DOCS, "a", false).await.unwrap_err(),
        ro.move_to(DOCS, "a", "b").await.unwrap_err(),
        ro.copy(DOCS, "a", "b").await.unwrap_err(),
    ];
    for e in refused {
        assert_eq!(e.kind, FileErrorKind::PermissionDenied, "{e}");
        assert!(e.message.contains("read-only"), "{e}");
    }
    assert_eq!(port.take(), ["read documents a", "list documents ", "info documents a"]);
    // the handle it came from still writes
    f.write(DOCS, "a", vec![]).await.unwrap();
    assert_eq!(port.take(), ["write documents a"]);
}

// ---- the person's view (the Files page) ---------------------------------------------------------------------

use crate::files::{FilesService, HostFiles, Listing, Place, UserFiles};
use crate::AppFiles;
use async_trait::async_trait;
use std::collections::BTreeSet;
use vos_domain::{
    limits::{MAX_LIST_ENTRIES, MAX_UPLOAD_BYTES},
    AppId, FileError, FileInfo, FileKind, FilePath, FileResult, HostPath,
};

/// Files that remember which paths exist, so "is something already there" can be asked; every call is recorded.
#[derive(Default)]
struct Shelf {
    there: parking_lot::Mutex<BTreeSet<String>>,
    calls: parking_lot::Mutex<Vec<String>>,
}

impl Shelf {
    fn with(paths: &[&str]) -> Arc<Self> {
        let shelf = Self::default();
        shelf.there.lock().extend(paths.iter().map(|p| (*p).to_owned()));
        Arc::new(shelf)
    }
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.lock())
    }
    fn note(&self, what: &str, at: Location, path: &FilePath) {
        self.calls.lock().push(format!("{what} {} {path}", at.as_str()));
    }
}

#[async_trait]
impl AppFiles for Shelf {
    async fn read(&self, at: Location, path: &FilePath) -> FileResult<Vec<u8>> {
        self.note("read", at, path);
        Ok(vec![])
    }
    async fn write(&self, at: Location, path: &FilePath, data: Vec<u8>) -> FileResult<()> {
        self.calls.lock().push(format!("write {} {path} ({} bytes)", at.as_str(), data.len()));
        self.there.lock().insert(path.to_string());
        Ok(())
    }
    async fn list(&self, at: Location, path: &FilePath) -> FileResult<Vec<FileInfo>> {
        self.note("list", at, path);
        Ok(vec![])
    }
    async fn info(&self, _: Location, path: &FilePath) -> FileResult<FileInfo> {
        if self.there.lock().contains(&path.to_string()) {
            Ok(FileInfo { name: path.to_string(), kind: FileKind::File, size: 0, modified: None })
        } else {
            Err(FileError::new(FileErrorKind::NotFound, "no such file or directory"))
        }
    }
    async fn create_dir(&self, at: Location, path: &FilePath) -> FileResult<()> {
        self.note("create_dir", at, path);
        Ok(())
    }
    async fn remove(&self, at: Location, path: &FilePath, recursive: bool) -> FileResult<()> {
        self.note(if recursive { "remove_all" } else { "remove" }, at, path);
        Ok(())
    }
    async fn move_to(&self, at: Location, from: &FilePath, to: &FilePath) -> FileResult<()> {
        self.calls.lock().push(format!("move {} {from} {to}", at.as_str()));
        Ok(())
    }
    async fn copy(&self, _: Location, _: &FilePath, _: &FilePath) -> FileResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn the_persons_view_of_a_place_reaches_only_documents_and_keeps_the_path_rules() {
    let shelf = Shelf::with(&[]);
    let files = UserFiles::of_app(shelf.clone());
    files.list("").await.unwrap();
    files.list("notes/").await.unwrap();
    files.read("notes/a.txt").await.unwrap();
    files.remove("notes", true).await.unwrap();
    files.remove("old.txt", false).await.unwrap();
    files.create_dir("new").await.unwrap();
    files.rename("a.txt", "b.txt").await.unwrap();
    files.upload("c.txt", vec![1, 2, 3], false).await.unwrap();
    assert_eq!(
        shelf.take(),
        [
            "list documents ",
            "list documents notes",
            "read documents notes/a.txt",
            // a directory goes with everything in it only when that was asked for
            "remove_all documents notes",
            "remove documents old.txt",
            "create_dir documents new",
            "move documents a.txt b.txt",
            "write documents c.txt (3 bytes)",
        ]
    );
    for bad in ["/etc/passwd", "../cache/x", "a/../../x", "a\\b"] {
        let kinds = [
            files.list(bad).await.unwrap_err().kind,
            files.read(bad).await.unwrap_err().kind,
            files.remove(bad, true).await.unwrap_err().kind,
            files.create_dir(bad).await.unwrap_err().kind,
            files.rename(bad, "ok").await.unwrap_err().kind,
            files.rename("ok", bad).await.unwrap_err().kind,
            files.upload(bad, vec![], true).await.unwrap_err().kind,
        ];
        assert!(kinds.iter().all(|k| *k == FileErrorKind::InvalidInput), "{bad:?}: {kinds:?}");
    }
    for root in ["", "."] {
        assert_eq!(
            files.remove(root, true).await.unwrap_err().kind,
            FileErrorKind::InvalidInput,
            "the place stays"
        );
        assert_eq!(files.create_dir(root).await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(files.upload(root, vec![], true).await.unwrap_err().kind, FileErrorKind::InvalidInput);
    }
    assert!(shelf.take().is_empty(), "none of that reached the port");
}

#[tokio::test]
async fn an_upload_never_replaces_a_file_unless_it_was_asked_to_and_has_a_size_limit() {
    let shelf = Shelf::with(&["report.csv"]);
    let files = UserFiles::of_app(shelf.clone());
    let refused = files.upload("report.csv", vec![1], false).await.unwrap_err();
    assert_eq!(refused.kind, FileErrorKind::AlreadyExists);
    assert!(refused.message.contains("report.csv"), "{refused}");
    assert!(shelf.take().is_empty(), "nothing was written");
    files.upload("report.csv", vec![1], true).await.unwrap();
    assert_eq!(shelf.take(), ["write documents report.csv (1 bytes)"]);

    let too_big = files.upload("big.bin", vec![0; MAX_UPLOAD_BYTES + 1], true).await.unwrap_err();
    assert_eq!(too_big.kind, FileErrorKind::TooLarge);
    assert!(shelf.take().is_empty());
    files.upload("big.bin", vec![0; MAX_UPLOAD_BYTES], false).await.unwrap();
}

#[tokio::test]
async fn a_new_folder_and_a_rename_never_take_a_name_that_is_in_use() {
    let shelf = Shelf::with(&["taken", "a.txt"]);
    let files = UserFiles::of_app(shelf.clone());
    assert_eq!(files.create_dir("taken").await.unwrap_err().kind, FileErrorKind::AlreadyExists);
    assert_eq!(files.rename("a.txt", "taken").await.unwrap_err().kind, FileErrorKind::AlreadyExists);
    assert!(shelf.take().is_empty());
    // onto itself is not a clash
    files.rename("a.txt", "./a.txt").await.unwrap();
    assert_eq!(shelf.take(), ["move documents a.txt a.txt"]);
}

/// A fake of the machine's file system: it records what reached it.
#[derive(Default)]
struct Machine(parking_lot::Mutex<Vec<String>>);

impl Machine {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock())
    }
    #[allow(clippy::unnecessary_wraps)] // the answer of every call that only records
    fn note(&self, line: String) -> FileResult<()> {
        self.0.lock().push(line);
        Ok(())
    }
}

#[async_trait]
impl HostFiles for Machine {
    fn home(&self) -> Option<HostPath> {
        HostPath::parse("/home/me").ok()
    }
    async fn list(&self, path: &HostPath, limit: usize) -> FileResult<Listing> {
        self.note(format!("list {path} (at most {limit})"))?;
        Ok(Listing::default())
    }
    async fn read(&self, path: &HostPath) -> FileResult<Vec<u8>> {
        self.note(format!("read {path}"))?;
        Ok(vec![])
    }
    async fn write(&self, path: &HostPath, data: Vec<u8>, replace: bool) -> FileResult<()> {
        self.note(format!("write {path} ({} bytes, replace: {replace})", data.len()))
    }
    async fn create_dir(&self, path: &HostPath) -> FileResult<()> {
        self.note(format!("create_dir {path}"))
    }
    async fn rename(&self, from: &HostPath, to: &HostPath) -> FileResult<()> {
        self.note(format!("rename {from} {to}"))
    }
    async fn remove(&self, path: &HostPath, recursive: bool) -> FileResult<()> {
        self.note(format!("remove {path} (recursive: {recursive})"))
    }
}

#[tokio::test]
async fn the_computer_takes_any_absolute_path_and_nothing_else() {
    let machine = Arc::new(Machine::default());
    let files = UserFiles::of_computer(machine.clone());
    files.list("").await.unwrap();
    files.list("/etc").await.unwrap();
    files.read("/home/me/../you/notes.txt").await.unwrap();
    files.upload("/tmp/a.bin", vec![1, 2], false).await.unwrap();
    files.upload("/tmp/a.bin", vec![1], true).await.unwrap();
    files.create_dir("/tmp/new").await.unwrap();
    files.rename("/tmp/a", "/var/tmp/b").await.unwrap();
    files.remove("/tmp/new", false).await.unwrap();
    files.remove("/tmp/tree", true).await.unwrap();
    assert_eq!(
        machine.take(),
        [
            format!("list / (at most {MAX_LIST_ENTRIES})"),
            format!("list /etc (at most {MAX_LIST_ENTRIES})"),
            "read /home/you/notes.txt".to_owned(),
            "write /tmp/a.bin (2 bytes, replace: false)".to_owned(),
            "write /tmp/a.bin (1 bytes, replace: true)".to_owned(),
            "create_dir /tmp/new".to_owned(),
            "rename /tmp/a /var/tmp/b".to_owned(),
            "remove /tmp/new (recursive: false)".to_owned(),
            "remove /tmp/tree (recursive: true)".to_owned(),
        ]
    );
    // a path here is absolute: a relative one is not guessed at
    for bad in ["notes.txt", "~/notes.txt", "C:/Users", "/a\0b"] {
        assert_eq!(files.list(bad).await.unwrap_err().kind, FileErrorKind::InvalidInput, "{bad:?}");
        assert_eq!(files.read(bad).await.unwrap_err().kind, FileErrorKind::InvalidInput, "{bad:?}");
        assert_eq!(files.upload(bad, vec![], true).await.unwrap_err().kind, FileErrorKind::InvalidInput);
    }
    // the top of the file system is listed, never written, made, renamed or removed
    for top in ["", "/", "/.."] {
        assert_eq!(files.remove(top, true).await.unwrap_err().kind, FileErrorKind::InvalidInput, "{top:?}");
        assert_eq!(files.create_dir(top).await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(files.upload(top, vec![], true).await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(files.rename(top, "/x").await.unwrap_err().kind, FileErrorKind::InvalidInput);
        assert_eq!(files.rename("/x", top).await.unwrap_err().kind, FileErrorKind::InvalidInput);
    }
    let too_big = files.upload("/tmp/big", vec![0; MAX_UPLOAD_BYTES + 1], true).await.unwrap_err();
    assert_eq!(too_big.kind, FileErrorKind::TooLarge);
    assert!(machine.take().is_empty(), "none of that reached the machine");
}

/// Files whose one directory holds more entries than a listing carries.
struct Crowded;
#[async_trait]
impl AppFiles for Crowded {
    async fn read(&self, _: Location, _: &FilePath) -> FileResult<Vec<u8>> {
        Ok(vec![])
    }
    async fn write(&self, _: Location, _: &FilePath, _: Vec<u8>) -> FileResult<()> {
        Ok(())
    }
    async fn list(&self, _: Location, _: &FilePath) -> FileResult<Vec<FileInfo>> {
        Ok((0..MAX_LIST_ENTRIES + 5)
            .map(|n| FileInfo { name: format!("{n:06}"), kind: FileKind::File, size: 0, modified: None })
            .collect())
    }
    async fn info(&self, _: Location, _: &FilePath) -> FileResult<FileInfo> {
        Err(FileError::new(FileErrorKind::NotFound, "no"))
    }
    async fn create_dir(&self, _: Location, _: &FilePath) -> FileResult<()> {
        Ok(())
    }
    async fn remove(&self, _: Location, _: &FilePath, _: bool) -> FileResult<()> {
        Ok(())
    }
    async fn move_to(&self, _: Location, _: &FilePath, _: &FilePath) -> FileResult<()> {
        Ok(())
    }
    async fn copy(&self, _: Location, _: &FilePath, _: &FilePath) -> FileResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_listing_of_an_apps_documents_is_cut_off_at_the_cap_and_says_so() {
    let listing = UserFiles::of_app(Arc::new(Crowded)).list("").await.unwrap();
    assert_eq!(listing.entries.len(), MAX_LIST_ENTRIES);
    assert!(listing.truncated);
    assert_eq!(listing.entries[0].info.name, "000000");
    let small = UserFiles::of_app(Shelf::with(&[])).list("").await.unwrap();
    assert!(!small.truncated);
}

#[tokio::test]
async fn a_place_is_an_installed_apps_documents_or_the_computer_when_the_host_offers_it() {
    let notes = AppId::parse("notes").unwrap();
    let installed = notes.clone();
    let store = Arc::new(RecordingFiles::default());
    let without = FilesService::new(store.clone(), move |app| *app == installed, None);
    assert!(!without.shares_computer() && without.home().is_none());
    assert!(without.open(&Place::App(notes.clone())).is_ok());
    let missing = without.open(&Place::App(AppId::parse("nope").unwrap())).err().unwrap();
    assert_eq!(missing.kind, FileErrorKind::NotFound);
    let off = without.open(&Place::Computer).err().unwrap();
    assert_eq!(off.kind, FileErrorKind::Unsupported);
    assert!(off.message.contains("host_files"), "{off}");

    let machine = Arc::new(Machine::default());
    let with = FilesService::new(store, |_| false, Some(machine.clone()));
    assert!(with.shares_computer());
    assert_eq!(with.home().unwrap().to_string(), "/home/me");
    with.open(&Place::Computer).unwrap().list("/home/me/Desktop").await.unwrap();
    assert_eq!(machine.take(), [format!("list /home/me/Desktop (at most {MAX_LIST_ENTRIES})")]);
}
