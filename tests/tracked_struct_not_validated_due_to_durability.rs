use salsa::{Durability, Setter};

#[salsa::db]
trait Db: salsa::Database {
    fn file(&self, idx: usize) -> File;
}

#[salsa::input]
struct File {
    field: usize,
}

#[salsa::tracked]
struct Definition<'db> {
    file: File,
}

#[salsa::tracked]
struct Index<'db> {
    definitions: Definitions<'db>,
}

#[salsa::tracked]
struct Definitions<'db> {
    definition: Definition<'db>,
}

#[salsa::tracked]
struct Inference<'db> {
    definition: Definition<'db>,
}

#[salsa::tracked]
fn index<'db>(db: &'db dyn Db, file: File) -> Index<'db> {
    let _ = file.field(db);
    Index::new(db, Definitions::new(db, Definition::new(db, file)))
}

#[salsa::tracked]
fn definitions<'db>(db: &'db dyn Db, file: File) -> Definitions<'db> {
    index(db, file).definitions(db)
}

#[salsa::tracked]
fn infer<'db>(db: &'db dyn Db, definition: Definition<'db>) -> Inference<'db> {
    let file = definition.file(db);
    if file.field(db) < 1 {
        let dependent_file = db.file(1);
        infer(db, definitions(db, dependent_file).definition(db))
    } else {
        db.file(0).field(db);
        Inference::new(db, definition)
    }
}

#[salsa::tracked]
fn check<'db>(db: &'db dyn Db, file: File) -> Inference<'db> {
    let defs = definitions(db, file);
    infer(db, defs.definition(db))
}

#[test]
fn execute() {
    #[salsa::db]
    #[derive(Default)]
    struct Database {
        storage: salsa::Storage<Self>,
        files: Vec<File>,
    }

    #[salsa::db]
    impl salsa::Database for Database {}

    #[salsa::db]
    impl Db for Database {
        fn file(&self, idx: usize) -> File {
            self.files[idx]
        }
    }

    let mut db = Database::default();
    // Create a file 0 with low durability, and a file 1 with high durability.

    let file0 = File::new(&db, 0);
    db.files.push(file0);

    let file1 = File::new(&db, 1);
    file1
        .set_field(&mut db)
        .with_durability(Durability::HIGH)
        .to(1);
    db.files.push(file1);

    // check(0) -> infer(0) -> definitions(0) -> index(0)
    //                     \-> infer(1) -> definitions(1) -> index(1)

    assert_eq!(check(&db, file0).definition(&db).file(&db).field(&db), 1);

    // update the low durability file 0
    file0.set_field(&mut db).to(0);

    // Re-query check(0). definitions(1) is high durability so it short-circuits in shallow-verify,
    // meaning we never verify index(1) at all, but index(1) created the tracked struct
    // Definition(1), so we never validate Definition(1) in R2, so when we try to verify
    // Definition.file(1) (as an input of infer(1) ) we hit a panic for trying to use a struct that
    // isn't validated in R2.
    check(&db, file0);
}
