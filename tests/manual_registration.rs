#![cfg(not(feature = "inventory"))]

mod ingredients {
    #[salsa::input]
    pub(super) struct MyInput {
        field: u32,
    }

    #[salsa::tracked]
    pub(super) struct MyTracked<'db> {
        pub(super) field: u32,
    }

    #[salsa::interned]
    pub(super) struct MyInterned<'db> {
        pub(super) field: u32,
    }

    #[salsa::tracked]
    pub(super) fn intern<'db>(db: &'db dyn salsa::Database, input: MyInput) -> MyInterned<'db> {
        MyInterned::new(db, input.field(db))
    }

    #[salsa::tracked]
    pub(super) fn track<'db>(db: &'db dyn salsa::Database, input: MyInput) -> MyTracked<'db> {
        MyTracked::new(db, input.field(db))
    }
}

mod generic {
    #[derive(salsa::InputData)]
    pub(super) struct Number(pub(super) u32);

    #[derive(Copy, Clone, Eq, Hash, PartialEq, salsa::Struct, salsa::Update)]
    pub(super) struct NumberHandle(salsa::Input<Number>);

    #[derive(Clone, Eq, Hash, PartialEq, salsa::InternedData, salsa::Update)]
    pub(super) struct Text(pub(super) String);

    #[derive(Copy, Clone, Eq, Hash, PartialEq, salsa::Struct, salsa::Update)]
    pub(super) struct TextHandle<'db>(salsa::Interned<'db, Text>);

    #[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
    #[derive(Hash, salsa::TrackedData, salsa::Update)]
    pub(super) struct Data(pub(super) u32);

    #[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
    pub(super) struct DataHandle<'db>(salsa::Tracked<'db, Data>);

    impl<'db> DataHandle<'db> {
        pub(super) fn data(self, db: &'db dyn salsa::Database) -> &'db Data {
            salsa::Tracked::fields(db, self)
        }
    }

    #[salsa::tracked]
    pub(super) fn track<'db>(db: &'db dyn salsa::Database, input: NumberHandle) -> DataHandle<'db> {
        salsa::Tracked::new(db, Data(salsa::Input::fields(db, input).0))
    }
}

#[salsa::db]
#[derive(Clone, Default)]
pub struct DatabaseImpl {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for DatabaseImpl {}

#[test]
fn single_database() {
    let db = DatabaseImpl {
        storage: salsa::Storage::builder()
            .ingredient::<ingredients::track>()
            .ingredient::<ingredients::intern>()
            .ingredient::<ingredients::MyInput>()
            .ingredient::<ingredients::MyTracked<'_>>()
            .ingredient::<ingredients::MyInterned<'_>>()
            .build(),
    };

    let input = ingredients::MyInput::new(&db, 1);

    let tracked = ingredients::track(&db, input);
    let interned = ingredients::intern(&db, input);

    assert_eq!(tracked.field(&db), 1);
    assert_eq!(interned.field(&db), 1);
}

#[test]
fn multiple_databases() {
    let db1 = DatabaseImpl {
        storage: salsa::Storage::builder()
            .ingredient::<ingredients::intern>()
            .ingredient::<ingredients::MyInput>()
            .ingredient::<ingredients::MyInterned<'_>>()
            .build(),
    };

    let input = ingredients::MyInput::new(&db1, 1);
    let interned = ingredients::intern(&db1, input);
    assert_eq!(interned.field(&db1), 1);

    // Create a second database with different ingredient indices.
    let db2 = DatabaseImpl {
        storage: salsa::Storage::builder()
            .ingredient::<ingredients::track>()
            .ingredient::<ingredients::intern>()
            .ingredient::<ingredients::MyInput>()
            .ingredient::<ingredients::MyTracked<'_>>()
            .ingredient::<ingredients::MyInterned<'_>>()
            .build(),
    };

    let input = ingredients::MyInput::new(&db2, 2);
    let interned = ingredients::intern(&db2, input);
    assert_eq!(interned.field(&db2), 2);

    let input = ingredients::MyInput::new(&db2, 3);
    let tracked = ingredients::track(&db2, input);
    assert_eq!(tracked.field(&db2), 3);
}

#[test]
fn generic_input() {
    let db = salsa::DatabaseImpl::builder()
        .ingredient::<generic::track>()
        .ingredient::<generic::NumberHandle>()
        .ingredient::<generic::DataHandle<'static>>()
        .ingredient::<generic::TextHandle<'static>>()
        .build();
    let input = salsa::Input::new(&db, generic::Number(22));
    let tracked = generic::track(&db, input);
    let interned = salsa::Interned::new(&db, generic::Text("main".to_owned()));

    assert_eq!(salsa::Input::fields(&db, input).0, 22);
    assert_eq!(tracked.data(&db).0, 22);
    assert_eq!(salsa::Interned::fields(&db, interned).0, "main");
}

fn panic_message(f: impl FnOnce()) -> String {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_err();
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
        .unwrap()
}

#[test]
fn missing_generic_registration_is_reported() {
    let db = salsa::DatabaseImpl::default();
    let message = panic_message(|| {
        let _ = salsa::Input::new(&db, generic::Number(0));
    });

    assert!(message.contains("Salsa input data `Number` is not registered in this database"));
    assert!(message.contains("derive(salsa::InputData)"));
}
