#![cfg(feature = "inventory")]

use salsa::{Database as _, DatabaseImpl};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

#[salsa::tracked]
fn tracked_with_args(_db: &dyn salsa::Database, left: u32, right: u32) -> u32 {
    left + right
}

#[salsa::input]
struct TraceInput {
    value: u32,
}

#[salsa::tracked]
fn populate_reusable_interned_arguments(db: &dyn salsa::Database, input: TraceInput) {
    input.value(db);
    tracked_with_args(db, 1, 2);
}

#[test]
#[should_panic(expected = "Cannot change database mid-query")]
fn different_database_panics_on_cold_query() {
    let db1 = DatabaseImpl::default();
    let db2 = DatabaseImpl::default();

    db1.attach(|_| tracked_with_args(&db2, 1, 2));
}

#[test]
#[should_panic(expected = "Cannot change database mid-query")]
fn different_database_panics_on_hot_query() {
    let db1 = DatabaseImpl::default();
    let db2 = DatabaseImpl::default();
    tracked_with_args(&db2, 1, 2);

    db1.attach(|_| tracked_with_args(&db2, 1, 2));
}

#[derive(Default)]
struct TraceState {
    next_span: AtomicU64,
    saw_attached_key: AtomicBool,
    saw_interned_arguments: AtomicBool,
    saw_raw_key: AtomicBool,
}

struct TraceSubscriber(Arc<TraceState>);

impl TraceSubscriber {
    fn record(&self, record: impl FnOnce(&mut FieldVisitor)) {
        let mut visitor = FieldVisitor(&self.0);
        record(&mut visitor);
    }
}

impl Subscriber for TraceSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        self.record(|visitor| attributes.record(visitor));
        Id::from_u64(self.0.next_span.fetch_add(1, Ordering::Relaxed) + 1)
    }

    fn record(&self, _span: &Id, values: &Record<'_>) {
        self.record(|visitor| values.record(visitor));
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        self.record(|visitor| event.record(visitor));
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

struct FieldVisitor<'a>(&'a TraceState);

impl Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let value = format!("{}={value:?}", field.name());
        if value.contains("Id(") {
            assert!(
                salsa::with_attached_database(|_| ()).is_some(),
                "database is not attached while recording {value}"
            );
            self.0.saw_attached_key.store(true, Ordering::Relaxed);
        }
        if value.contains("tracked_with_args::interned_arguments") {
            self.0.saw_interned_arguments.store(true, Ordering::Relaxed);
        }
        if value.contains("DatabaseKeyIndex(") {
            self.0.saw_raw_key.store(true, Ordering::Relaxed);
        }
    }
}

#[test]
fn tracing_hot_query_attaches_database() {
    let db = DatabaseImpl::default();
    let input = TraceInput::new(&db, 0);
    populate_reusable_interned_arguments(&db, input);

    let state = Arc::new(TraceState::default());
    let dispatch = tracing::Dispatch::new(TraceSubscriber(state.clone()));

    tracing::dispatcher::with_default(&dispatch, || {
        tracked_with_args(&db, 1, 2);
    });

    assert!(state.saw_attached_key.load(Ordering::Relaxed));
    assert!(state.saw_interned_arguments.load(Ordering::Relaxed));
    assert!(!state.saw_raw_key.load(Ordering::Relaxed));
}
