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
        assert!(e.message.contains("readOnly"), "{e}");
    }
    assert_eq!(port.take(), ["read documents a", "list documents ", "info documents a"]);
    // the handle it came from still writes
    f.write(DOCS, "a", vec![]).await.unwrap();
    assert_eq!(port.take(), ["write documents a"]);
}
