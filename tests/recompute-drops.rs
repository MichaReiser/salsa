#![cfg(feature = "inventory")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use salsa::Setter;

#[salsa::input]
struct Input {
    #[returns(copy)]
    value: u32,
}

#[salsa::tracked]
fn recompute(db: &dyn salsa::Database, input: Input) -> DroppedValue {
    DroppedValue(input.value(db))
}

#[test]
fn recomputation_drops_replaced_values_without_blocking_readers() {
    DROPS.store(0, Ordering::SeqCst);

    let mut db = salsa::DatabaseImpl::new();
    let input = Input::new(&db, 0);
    let revision_trigger = Input::new(&db, 0);

    assert_eq!(recompute(&db, input).0, 0);
    input.set_value(&mut db).to(1);
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);

    assert_eq!(recompute(&db, input).0, 1);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);

    // Starting another revision must not drop the replaced output a second time.
    revision_trigger.set_value(&mut db).to(1);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);

    // Revalidating an unchanged memo keeps its output alive.
    assert_eq!(recompute(&db, input).0, 1);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);

    input.set_value(&mut db).to(2);
    let (drop_started, release_drop) = block_drop_of(1);
    let recompute_db = db.clone();
    let recomputing_thread = std::thread::spawn(move || recompute(&recompute_db, input).0);

    let drop_started_result = drop_started.recv_timeout(Duration::from_secs(5));

    let reader_db = db.clone();
    let (reader_result_tx, reader_result_rx) = mpsc::channel();
    let reader_thread = std::thread::spawn(move || {
        reader_result_tx
            .send(recompute(&reader_db, input).0)
            .unwrap();
    });
    let reader_result = reader_result_rx.recv_timeout(Duration::from_secs(5));

    // Always unblock the destructor before asserting, so failures cannot strand a thread.
    let _ = release_drop.send(());
    let recomputed_value = recomputing_thread.join().unwrap();
    reader_thread.join().unwrap();

    drop_started_result.expect("the previous value was not dropped during recomputation");
    assert_eq!(
        reader_result.expect("reader remained blocked by the destructor"),
        2
    );
    assert_eq!(recomputed_value, 2);
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
}

#[derive(PartialEq, salsa::Update)]
struct DroppedValue(u32);

impl Drop for DroppedValue {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);

        let Some(blocked_drop) = BLOCKED_DROP.lock().unwrap().take() else {
            return;
        };
        assert_eq!(blocked_drop.value, self.0);

        blocked_drop.started.send(()).unwrap();
        blocked_drop.release.recv().unwrap();
    }
}

struct BlockedDrop {
    value: u32,
    started: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

static DROPS: AtomicUsize = AtomicUsize::new(0);
static BLOCKED_DROP: Mutex<Option<BlockedDrop>> = Mutex::new(None);

fn block_drop_of(value: u32) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *BLOCKED_DROP.lock().unwrap() = Some(BlockedDrop {
        value,
        started: started_tx,
        release: release_rx,
    });
    (started_rx, release_tx)
}
