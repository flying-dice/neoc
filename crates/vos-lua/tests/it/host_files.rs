#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! "This Computer": the machine's own file system through the person's use case (`UserFiles`) and the real adapter
//! (`LocalFiles`), in a temporary directory reached by its absolute path. Nothing is confined: what is checked is that
//! it behaves as a file manager does, and that the operating system's refusals come back as what they are.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use vos_application::{HostFiles, Listing, UserFiles};
use vos_domain::{FileErrorKind, FileKind, HostPath};
use vos_lua::LocalFiles;

struct World {
    tmp: TempDir,
    files: UserFiles,
}

impl World {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("docs/2026")).unwrap();
        std::fs::write(tmp.path().join("docs/2026/todo.txt"), "buy milk").unwrap();
        std::fs::write(tmp.path().join("report.csv"), "a,b\n").unwrap();
        Self { tmp, files: UserFiles::of_computer(std::sync::Arc::new(LocalFiles::new(None))) }
    }
    fn dir(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }
    /// The absolute wire path of `inner` in the temporary directory.
    fn at(&self, inner: &str) -> String {
        format!("{}/{inner}", wire(&self.dir())).trim_end_matches('/').to_owned()
    }
}

/// A native absolute path as the wire has it: `/` between names and at the start (`/C:/Users/me` on Windows, whose
/// `canonicalize` answers the long form `\\?\C:\...`).
fn wire(native: &Path) -> String {
    let text = native.to_str().unwrap();
    let text = text.strip_prefix(r"\\?\").unwrap_or(text).replace('\\', "/");
    format!("/{}", text.trim_start_matches('/'))
}

fn names(listing: &Listing) -> Vec<&str> {
    listing.entries.iter().map(|e| e.info.name.as_str()).collect()
}

#[tokio::test]
async fn any_absolute_path_is_listed_read_written_renamed_and_removed() {
    let w = World::new();
    assert_eq!(names(&w.files.list(&w.at("")).await.unwrap()), ["docs", "report.csv"]);
    assert_eq!(names(&w.files.list(&w.at("docs/2026")).await.unwrap()), ["todo.txt"]);
    // `..` is a step up, as in any path
    assert_eq!(names(&w.files.list(&w.at("docs/2026/../..")).await.unwrap()), ["docs", "report.csv"]);
    assert_eq!(w.files.read(&w.at("docs/2026/todo.txt")).await.unwrap(), b"buy milk");
    let listed = w.files.list(&w.at("docs/2026")).await.unwrap();
    let todo = &listed.entries[0];
    assert_eq!((todo.info.kind, todo.info.size, todo.link), (FileKind::File, 8, false));
    assert!(todo.info.modified.unwrap() > 1.6e9);

    // the top of the file system is a directory like any other
    let top = w.files.list("/").await.unwrap();
    assert!(!top.entries.is_empty() && !top.truncated);
    assert_eq!(w.files.list("").await.unwrap().entries.len(), top.entries.len());

    w.files.create_dir(&w.at("new folder")).await.unwrap();
    assert!(w.dir().join("new folder").is_dir());
    assert_eq!(w.files.create_dir(&w.at("new folder")).await.unwrap_err().kind, FileErrorKind::AlreadyExists);
    assert_eq!(w.files.create_dir(&w.at("no/such/parent")).await.unwrap_err().kind, FileErrorKind::NotFound);

    w.files.rename(&w.at("report.csv"), &w.at("docs/report-2026.csv")).await.unwrap();
    assert!(w.dir().join("docs/report-2026.csv").is_file() && !w.dir().join("report.csv").exists());
    let clash = w.files.rename(&w.at("docs/report-2026.csv"), &w.at("docs/2026")).await.unwrap_err();
    assert_eq!(clash.kind, FileErrorKind::AlreadyExists, "nothing is replaced by a rename");

    // a folder that is not empty goes only when that was asked for
    let kept = w.files.remove(&w.at("docs"), false).await.unwrap_err();
    assert_eq!(kept.kind, FileErrorKind::NotEmpty);
    assert!(w.dir().join("docs/2026/todo.txt").is_file());
    w.files.remove(&w.at("new folder"), false).await.unwrap(); // an empty one goes
    w.files.remove(&w.at("docs/report-2026.csv"), false).await.unwrap();
    w.files.remove(&w.at("docs"), true).await.unwrap();
    assert!(!w.dir().join("docs").exists());
    assert_eq!(w.files.remove(&w.at("docs"), true).await.unwrap_err().kind, FileErrorKind::NotFound);
    assert_eq!(w.files.read(&w.at("nowhere.txt")).await.unwrap_err().kind, FileErrorKind::NotFound);
    assert_eq!(w.files.list(&w.at("nowhere")).await.unwrap_err().kind, FileErrorKind::NotFound);
}

#[tokio::test]
async fn an_upload_is_whole_or_absent_and_replaces_only_when_asked() {
    let w = World::new();
    let bytes: Vec<u8> = (0..=255).cycle().take(100_000).collect();
    w.files.upload(&w.at("docs/photo.bin"), bytes.clone(), false).await.unwrap();
    assert_eq!(std::fs::read(w.dir().join("docs/photo.bin")).unwrap(), bytes);
    assert_eq!(
        names(&w.files.list(&w.at("docs")).await.unwrap()),
        ["2026", "photo.bin"],
        "nothing temporary is left"
    );

    let kept = w.files.upload(&w.at("docs/photo.bin"), vec![9], false).await.unwrap_err();
    assert_eq!(kept.kind, FileErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(w.dir().join("docs/photo.bin")).unwrap(), bytes, "not replaced");
    assert_eq!(
        names(&w.files.list(&w.at("docs")).await.unwrap()),
        ["2026", "photo.bin"],
        "and nothing left behind"
    );
    w.files.upload(&w.at("docs/photo.bin"), vec![9], true).await.unwrap();
    assert_eq!(std::fs::read(w.dir().join("docs/photo.bin")).unwrap(), [9]);

    // never over a folder, never into a folder that is not there
    assert_eq!(
        w.files.upload(&w.at("docs/2026"), vec![1], false).await.unwrap_err().kind,
        FileErrorKind::AlreadyExists
    );
    assert_eq!(
        w.files.upload(&w.at("docs/2026"), vec![1], true).await.unwrap_err().kind,
        FileErrorKind::IsDirectory
    );
    assert_eq!(
        w.files.upload(&w.at("nowhere/a.txt"), vec![1], false).await.unwrap_err().kind,
        FileErrorKind::NotFound
    );
    assert!(!w.dir().join("nowhere").exists());
    assert_eq!(w.files.read(&w.at("docs")).await.unwrap_err().kind, FileErrorKind::IsDirectory);
}

#[cfg(unix)]
#[tokio::test]
async fn a_symbolic_link_is_followed_and_shown_as_a_link() {
    use std::os::unix::fs::symlink;
    let w = World::new();
    let elsewhere = TempDir::new().unwrap();
    std::fs::write(elsewhere.path().join("far.txt"), "far away").unwrap();
    symlink(elsewhere.path(), w.dir().join("shortcut")).unwrap();
    symlink(w.dir().join("docs/2026/todo.txt"), w.dir().join("todo-link.txt")).unwrap();

    let top = w.files.list(&w.at("")).await.unwrap();
    let entry = |name: &str| top.entries.iter().find(|e| e.info.name == name).unwrap();
    // a link says it is one, and is what it leads to
    assert_eq!((entry("shortcut").info.kind, entry("shortcut").link), (FileKind::Dir, true));
    assert_eq!((entry("todo-link.txt").info.kind, entry("todo-link.txt").link), (FileKind::File, true));
    assert_eq!(entry("todo-link.txt").info.size, 8);
    assert_eq!((entry("docs").info.kind, entry("docs").link), (FileKind::Dir, false));
    assert!(top.entries.iter().all(|e| e.problem.is_none()));

    // into a linked folder, wherever it leads, like any file manager
    assert_eq!(names(&w.files.list(&w.at("shortcut")).await.unwrap()), ["far.txt"]);
    assert_eq!(w.files.read(&w.at("shortcut/far.txt")).await.unwrap(), b"far away");
    assert_eq!(w.files.read(&w.at("todo-link.txt")).await.unwrap(), b"buy milk");
    w.files.upload(&w.at("shortcut/new.txt"), b"through the link".to_vec(), false).await.unwrap();
    assert_eq!(std::fs::read_to_string(elsewhere.path().join("new.txt")).unwrap(), "through the link");

    // removing a link removes the link, never what it leads to, even when asked to take everything
    w.files.remove(&w.at("shortcut"), true).await.unwrap();
    assert!(std::fs::symlink_metadata(w.dir().join("shortcut")).is_err());
    assert!(elsewhere.path().join("far.txt").is_file());
    w.files.remove(&w.at("todo-link.txt"), false).await.unwrap();
    assert!(w.dir().join("docs/2026/todo.txt").is_file());
}

#[cfg(unix)]
#[tokio::test]
async fn a_broken_link_and_a_link_in_a_circle_are_entries_with_a_reason_not_a_failed_listing() {
    use std::os::unix::fs::symlink;
    let w = World::new();
    symlink(w.dir().join("gone.txt"), w.dir().join("broken")).unwrap();
    symlink(w.dir().join("loop-b"), w.dir().join("loop-a")).unwrap();
    symlink(w.dir().join("loop-a"), w.dir().join("loop-b")).unwrap();

    let top = w.files.list(&w.at("")).await.unwrap();
    assert_eq!(names(&top), ["broken", "docs", "loop-a", "loop-b", "report.csv"], "the others are all there");
    let entry = |name: &str| top.entries.iter().find(|e| e.info.name == name).unwrap();
    assert_eq!((entry("broken").info.kind, entry("broken").link), (FileKind::Symlink, true));
    assert!(
        entry("broken").problem.as_deref().unwrap().contains("not there"),
        "{:?}",
        entry("broken").problem
    );
    assert_eq!(entry("loop-a").info.kind, FileKind::Symlink);
    assert!(entry("loop-a").problem.as_deref().unwrap().contains("cannot be followed"));
    assert!(entry("docs").problem.is_none());

    // opening one is an error about that entry
    assert_eq!(w.files.read(&w.at("broken")).await.unwrap_err().kind, FileErrorKind::NotFound);
    assert!(w.files.list(&w.at("loop-a")).await.is_err());
    assert!(w.files.read(&w.at("loop-a/x")).await.is_err());
    // and they can be cleared away
    for link in ["broken", "loop-a", "loop-b"] {
        w.files.remove(&w.at(link), false).await.unwrap();
    }
    assert_eq!(names(&w.files.list(&w.at("")).await.unwrap()), ["docs", "report.csv"]);
}

#[cfg(unix)]
#[tokio::test]
async fn a_folder_the_user_may_not_open_says_so_and_the_one_around_it_still_lists() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    let locked = w.dir().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("inside.txt"), "x").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let restore = || std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    if std::fs::read_dir(&locked).is_ok() {
        restore(); // running as root: nothing is denied, and there is nothing to check
        return;
    }

    // the folder is listed where it is
    let top = w.files.list(&w.at("")).await.unwrap();
    let entry = top.entries.iter().find(|e| e.info.name == "locked").unwrap();
    assert_eq!(entry.info.kind, FileKind::Dir);
    // opening it, and anything in it, is the operating system's refusal, said as such
    let refused = w.files.list(&w.at("locked")).await.unwrap_err();
    assert_eq!(refused.kind, FileErrorKind::PermissionDenied);
    assert_eq!(refused.message, "locked: You do not have permission to open this folder.");
    let read = w.files.read(&w.at("locked/inside.txt")).await.unwrap_err();
    assert_eq!(read.kind, FileErrorKind::PermissionDenied);
    assert!(read.message.contains("permission to open this file"), "{read}");
    let put = w.files.upload(&w.at("locked/new.txt"), vec![1], false).await.unwrap_err();
    assert_eq!(put.kind, FileErrorKind::PermissionDenied);
    assert!(put.message.contains("permission to save a file here"), "{put}");
    let made = w.files.create_dir(&w.at("locked/sub")).await.unwrap_err();
    assert_eq!(made.kind, FileErrorKind::PermissionDenied);
    let removed = w.files.remove(&w.at("locked"), true).await.unwrap_err();
    assert_eq!(removed.kind, FileErrorKind::PermissionDenied);
    restore();
    assert!(locked.join("inside.txt").is_file(), "nothing was half-removed");
}

#[tokio::test]
async fn a_listing_stops_at_its_cap_and_says_there_was_more() {
    let w = World::new();
    let many = w.dir().join("many");
    std::fs::create_dir(&many).unwrap();
    for n in 0..25 {
        std::fs::write(many.join(format!("file-{n:02}.txt")), "").unwrap();
    }
    let host = LocalFiles::new(None);
    let path = HostPath::parse(&w.at("many")).unwrap();
    let capped = host.list(&path, 10).await.unwrap();
    assert_eq!(capped.entries.len(), 10);
    assert!(capped.truncated);
    let mut sorted = names(&capped);
    sorted.sort_unstable();
    assert_eq!(names(&capped), sorted, "what is sent is sorted by name");
    let exact = host.list(&path, 25).await.unwrap();
    assert_eq!((exact.entries.len(), exact.truncated), (25, false));
    // through the use case the cap is the host's limit
    let all = w.files.list(&w.at("many")).await.unwrap();
    assert_eq!((all.entries.len(), all.truncated), (25, false));
}

#[test]
fn the_home_directory_is_offered_as_a_path_to_list() {
    let (native, wire) =
        if cfg!(windows) { (r"C:\Users\me", "/C:/Users/me") } else { ("/home/me", "/home/me") };
    assert_eq!(LocalFiles::new(Some(Path::new(native))).home().unwrap().to_string(), wire);
    // the long form of a Windows path is the same path
    if cfg!(windows) {
        assert_eq!(LocalFiles::new(Some(Path::new(r"\\?\C:\Users\me"))).home().unwrap().to_string(), wire);
    }
    assert!(LocalFiles::new(None).home().is_none());
    assert!(LocalFiles::new(Some(Path::new("relative/home"))).home().is_none());
}
