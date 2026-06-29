#![cfg(feature = "inventory")]

//! Test that a setting a field on a `#[salsa::input]`
//! overwrites and returns the old value.

use salsa::{Database, DatabaseImpl, Update};
use test_log::test;

#[salsa::input(debug)]
struct MyInput {
    field: String,
}

#[salsa::tracked(debug)]
struct MyTracked<'db> {
    data: salsa::TrackedField<MyInput>,
    next: salsa::TrackedField<MyList<'db>>,
}

#[derive(PartialEq, Eq, Clone, Debug, Update)]
enum MyList<'db> {
    None,
    Next(MyTracked<'db>),
}

#[salsa::tracked]
fn create_tracked_list(db: &dyn Database, input: MyInput) -> MyTracked<'_> {
    let t0 = MyTracked::new(db, input, MyList::None);
    MyTracked::new(db, input, MyList::Next(t0))
}

#[test]
fn execute() {
    DatabaseImpl::new().attach(|db| {
        let input = MyInput::new(db, "foo".to_string());
        let t0: MyTracked = create_tracked_list(db, input);
        let t1 = create_tracked_list(db, input);
        expect_test::expect![[r#"
            Tracked(
                Id(401),
                MyTracked {
                    data: Input(
                        Id(0),
                        MyInput {
                            field: "foo",
                        },
                    ),
                    next: Next(
                        Tracked(
                            Id(400),
                            MyTracked {
                                data: Input(
                                    Id(0),
                                    MyInput {
                                        field: "foo",
                                    },
                                ),
                                next: None,
                            },
                        ),
                    ),
                },
            )
        "#]]
        .assert_debug_eq(&t0);
        assert_eq!(t0, t1);
    })
}
