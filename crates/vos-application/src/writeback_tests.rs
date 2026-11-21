//! Two-way write-back against the in-memory fakes: baseRev, merge, human-wins, strict sources, revisions, undo.

use crate::{
    data::{Params, QueryInfo},
    live::{Conflict, SourceDef, SourceId, SourceKind},
    services::AppService,
    testing::Fakes,
    AppError, BindingKind, PortError, Resolution, WriteRequest,
};
use vos_domain::{Actor, AppId, RevisionReason};
use serde_json::json;

fn note_source(conflict: Conflict, writable: Option<Vec<String>>, rev: bool) -> SourceDef {
    SourceDef {
        id: SourceId("note".into()),
        kind: SourceKind::Row {
            key: "id".into(),
            rev: rev.then(|| "rev".into()),
            writable,
            has_write: true,
            conflict,
        },
        sql: "select id, body, rev from notes where id = :id".into(),
        info: QueryInfo {
            tables: ["notes".to_owned()].into(),
            columns: vec!["id".into(), "body".into(), "rev".into()],
            named_params: vec!["id".into()],
            read_only: true,
        },
    }
}

struct Setup {
    f: Fakes,
    svc: AppService,
    id: AppId,
}

/// A note whose body is `body` at revision 3; the app's write callback bumps the revision to 4.
async fn writable_note(def: SourceDef, body: &str) -> Setup {
    let f = Fakes::new();
    f.runtime.set_sources("notes", vec![def]);
    let svc = f.service();
    let id = svc.install_from_url(&f.add_app("notes", &[("t", "x")])).await.unwrap();
    let data = f.store.data("notes");
    data.set_rows("from notes where id", vec![json!({"id": 7, "body": body, "rev": 3})]);
    let d = data.clone();
    f.runtime.on_write(move |_, _, field, value| {
        d.set_rows("from notes where id", vec![json!({"id": 7, field: value, "rev": 4})]);
    });
    Setup { f, svc, id }
}

fn text_note() -> SourceDef {
    note_source(Conflict::HumanWins, None, true)
}

fn write(base_rev: Option<i64>, value: &str, binding: BindingKind) -> WriteRequest {
    WriteRequest {
        source: SourceId("note".into()),
        params: Params::named([("id", json!(7))]),
        key: json!(7),
        field: "body".into(),
        value: json!(value),
        base_rev: base_rev.map(Into::into),
        binding,
        actor: Actor::user("You"),
    }
}

fn text(base: &str) -> BindingKind {
    BindingKind::Text { base: Some(base.into()) }
}

#[tokio::test]
async fn a_write_on_the_current_revision_is_accepted_and_returns_the_new_revision() {
    let s = writable_note(text_note(), "milk").await;
    let ack = s.svc.write_field(&s.id, write(Some(3), "Milk", text("milk"))).await.unwrap();
    assert_eq!((ack.resolution, ack.rev, ack.value), (Resolution::Accepted, Some(json!(4)), json!("Milk")));
    assert!(ack.revision_id.is_none() && ack.lost.is_none());
    let w = s.f.runtime.writes();
    assert_eq!(
        (w[0].2.as_str(), &w[0].3, w[0].4.actor.clone(), w[0].4.now_ms),
        ("body", &json!("Milk"), Actor::user("You"), 1_000)
    );
}

#[tokio::test]
async fn a_stale_text_write_with_non_overlapping_edits_is_merged_and_nothing_is_lost() {
    // the spec's trace 2.8-B: the agent appended while the user capitalised
    let s = writable_note(text_note(), "milk\n- buy oat milk").await;
    let ack = s.svc.write_field(&s.id, write(Some(2), "Milk", text("milk"))).await.unwrap();
    assert_eq!(ack.resolution, Resolution::Merged);
    assert_eq!(ack.value, json!("Milk\n- buy oat milk"), "the client adopts the merged text");
    assert_eq!(s.f.runtime.writes()[0].3, json!("Milk\n- buy oat milk"));
    assert!(ack.revision_id.is_none(), "a clean merge keeps no revision");
}

#[tokio::test]
async fn overlapping_text_edits_go_to_the_human_and_the_agents_text_is_kept_for_undo() {
    let s = writable_note(text_note(), "buy bread today").await;
    let ack =
        s.svc.write_field(&s.id, write(Some(2), "buy cheese today", text("buy milk today"))).await.unwrap();
    assert_eq!(ack.resolution, Resolution::Replaced);
    assert_eq!(ack.value, json!("buy cheese today"));
    assert_eq!(ack.lost, Some(json!("buy bread today")));
    let id = ack.revision_id.expect("a revision id for Undo");

    let kept = s.svc.revisions(&s.id, "note", &json!(7), "body").await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, id);
    assert_eq!(kept[0].revision.reason, RevisionReason::TextConflict);
    assert_eq!(kept[0].revision.value, json!("buy bread today"));
    assert_eq!(kept[0].revision.replaced_by, Actor::user("You"));
    assert_eq!(kept[0].revision.recorded_at_ms, 1_000);

    // Undo brings the agent's text back, and keeps the human's so the undo can itself be undone
    let undone = s
        .svc
        .restore_revision(&s.id, id, Params::named([("id", json!(7))]), Actor::user("You"))
        .await
        .unwrap();
    assert_eq!((undone.resolution, undone.value.clone()), (Resolution::Replaced, json!("buy bread today")));
    let kept = s.svc.revisions(&s.id, "note", &json!(7), "body").await.unwrap();
    assert_eq!(kept.len(), 2);
    assert_eq!(
        (kept[0].revision.reason, kept[0].revision.value.clone()),
        (RevisionReason::Restored, json!("buy cheese today"))
    );
}

#[tokio::test]
async fn a_stale_text_write_without_a_merge_base_replaces_and_keeps_theirs() {
    // e.g. after a host restart the ring of sent values is gone
    let s = writable_note(text_note(), "theirs").await;
    let ack =
        s.svc.write_field(&s.id, write(Some(2), "mine", BindingKind::Text { base: None })).await.unwrap();
    assert_eq!((ack.resolution, ack.lost), (Resolution::Replaced, Some(json!("theirs"))));
    assert_eq!(ack.value, json!("mine"));
}

#[tokio::test]
async fn a_stale_scalar_write_wins_and_the_overwritten_value_is_kept() {
    let s = writable_note(text_note(), "theirs").await;
    let ack = s.svc.write_field(&s.id, write(Some(2), "mine", BindingKind::Scalar)).await.unwrap();
    assert_eq!(ack.resolution, Resolution::Replaced);
    let kept = s.svc.revisions(&s.id, "note", &json!(7), "body").await.unwrap();
    assert_eq!(
        (kept[0].revision.reason, kept[0].revision.value.clone()),
        (RevisionReason::Overwritten, json!("theirs"))
    );
}

#[tokio::test]
async fn a_stale_write_that_equals_the_current_value_is_just_accepted() {
    let s = writable_note(text_note(), "same").await;
    let ack = s.svc.write_field(&s.id, write(Some(2), "same", BindingKind::Scalar)).await.unwrap();
    assert_eq!(ack.resolution, Resolution::Accepted);
    assert!(ack.revision_id.is_none());
}

#[tokio::test]
async fn a_stale_write_to_a_strict_source_is_rejected_with_the_current_value() {
    let s = writable_note(note_source(Conflict::Reject, None, true), "milk").await;
    let ack = s.svc.write_field(&s.id, write(Some(2), "Mine", BindingKind::Scalar)).await.unwrap();
    assert_eq!((ack.resolution, ack.rev, ack.value), (Resolution::Rejected, Some(json!(3)), json!("milk")));
    assert!(s.f.runtime.writes().is_empty(), "the app's write callback never ran");
}

#[tokio::test]
async fn without_a_revision_column_there_is_nothing_to_be_stale_against() {
    // the client sends baseRev 0 for a source with no `rev` column
    let s = writable_note(note_source(Conflict::Reject, None, false), "milk").await;
    let ack = s.svc.write_field(&s.id, write(Some(0), "Milk", text("milk"))).await.unwrap();
    assert_eq!(ack.resolution, Resolution::Accepted);
    assert_eq!(ack.rev, None);
}

#[tokio::test]
async fn writes_to_read_only_fields_missing_rows_and_wrong_keys_are_rejected() {
    let s = writable_note(note_source(Conflict::HumanWins, Some(vec!["title".into()]), true), "x").await;
    let err = s.svc.write_field(&s.id, write(Some(3), "x", BindingKind::Scalar)).await.unwrap_err();
    assert!(err.to_string().contains("not writable"), "{err}");

    let s = writable_note(text_note(), "x").await;
    let mut wrong_key = write(Some(3), "x", BindingKind::Scalar);
    wrong_key.key = json!(8);
    assert!(s.svc.write_field(&s.id, wrong_key).await.unwrap_err().to_string().contains("key"));
    s.f.store.data("notes").set_rows("from notes where id", vec![]);
    assert!(matches!(
        s.svc.write_field(&s.id, write(Some(3), "x", BindingKind::Scalar)).await,
        Err(AppError::Port(PortError::NotFound(_)))
    ));
    let mut unknown = write(Some(3), "x", BindingKind::Scalar);
    unknown.source = SourceId("nope".into());
    assert!(matches!(s.svc.write_field(&s.id, unknown).await, Err(AppError::Port(PortError::NotFound(_)))));
}

#[tokio::test]
async fn expired_revisions_are_purged_after_thirty_days() {
    let s = writable_note(text_note(), "theirs").await;
    s.svc.write_field(&s.id, write(Some(2), "mine", BindingKind::Scalar)).await.unwrap();
    assert_eq!(s.svc.purge_expired_revisions().await, 0, "fresh");
    s.f.clock.advance(30 * 24 * 60 * 60 * 1000 + 1);
    assert_eq!(s.svc.purge_expired_revisions().await, 1);
    assert!(s.svc.revisions(&s.id, "note", &json!(7), "body").await.unwrap().is_empty());
}
