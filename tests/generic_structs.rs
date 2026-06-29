#![cfg(feature = "inventory")]

use std::borrow::{Borrow, ToOwned};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};

use salsa::{HashEqLike, Input, InputField, Interned, Lookup, Setter, Tracked, TrackedField};

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Debug, Hash, PartialEq, salsa::Update)]
struct NonCloneValue(u32);

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Debug, salsa::InputData)]
struct Options {
    debug: InputField<bool>,
    target: InputField<String>,
    source: InputField<u32>,
    other_source: InputField<u32>,
    custom: InputField<NonCloneValue>,
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
struct OptionsHandle(Input<Options>);

impl OptionsHandle {
    fn create(db: &salsa::DatabaseImpl) -> Self {
        options(db)
    }

    fn data(self, db: &dyn salsa::Database) -> &Options {
        Input::fields(db, self)
    }
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Debug, Hash, salsa::TrackedData, salsa::Update)]
struct Node {
    kind: u32,
    ty: TrackedField<u32>,
    span: TrackedField<u32>,
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
struct NodeHandle<'db>(Tracked<'db, Node>);

impl<'db> NodeHandle<'db> {
    fn data(self, db: &'db dyn salsa::Database) -> &'db Node {
        Tracked::fields(db, self)
    }
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Debug, Hash, salsa::TrackedData, salsa::Update)]
struct NonCloneNode {
    value: TrackedField<NonCloneValue>,
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
struct NonCloneNodeHandle<'db>(Tracked<'db, NonCloneNode>);

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::InternedData, salsa::Update)]
struct NameData {
    text: String,
    namespace: u32,
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::InternedData, salsa::Update)]
struct SymbolData(u32);

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
struct Symbol<'db>(Interned<'db, SymbolData>);

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Debug, PartialEq, Eq, Hash, salsa::Update)]
struct TextKey(String);

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Debug, PartialEq, Eq, Hash, salsa::InternedData, salsa::Update)]
struct Text(TextKey);

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
struct TextHandle<'db>(Interned<'db, Text>);

impl Clone for Text {
    fn clone(&self) -> Self {
        Self(TextKey(self.0.0.clone()))
    }
}

impl Borrow<TextKey> for Text {
    fn borrow(&self) -> &TextKey {
        &self.0
    }
}

impl ToOwned for TextKey {
    type Owned = Text;

    fn to_owned(&self) -> Self::Owned {
        Text(TextKey(self.0.clone()))
    }
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
struct Name<'db>(Interned<'db, NameData>);

impl<'db> Name<'db> {
    fn intern(db: &'db dyn salsa::Database, text: &str, namespace: u32) -> Self {
        Interned::new(
            db,
            NameData {
                text: text.to_owned(),
                namespace,
            },
        )
    }

    fn data(self, db: &'db dyn salsa::Database) -> &'db NameData {
        Interned::fields(db, self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Supertype, salsa::Update)]
enum GenericInterned<'db> {
    Name(Name<'db>),
    Symbol(Symbol<'db>),
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::InternedData, salsa::Update)]
struct ScopedNameData<'db> {
    namespace: Name<'db>,
    text: String,
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
struct ScopedName<'db>(Interned<'db, ScopedNameData<'db>>);

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Debug, Hash, salsa::TrackedData, salsa::Update)]
struct ScopedNodeData<'db> {
    name: Name<'db>,
    ty: TrackedField<u32>,
}

#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
struct ScopedNode<'db>(Tracked<'db, ScopedNodeData<'db>>);

#[salsa::input(fields = MacroInputFields)]
#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
struct MacroInput {
    value: u32,
}

impl MacroInputFields {
    fn doubled(&self) -> u32 {
        self.value * 2
    }
}

// `data` remains accepted for compatibility; it now names the retained fields type.
#[salsa::interned(data = MacroInternedFields)]
#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
struct MacroInterned<'db> {
    text: String,
}

impl MacroInternedFields {
    fn len(&self) -> usize {
        self.text.len()
    }
}

#[salsa::tracked(fields = MacroTrackedFields)]
#[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
struct MacroTracked<'db> {
    key: u32,
    value: salsa::TrackedField<u32>,
}

impl MacroTrackedFields {
    fn key_plus_value(&self, db: &dyn salsa::Database) -> u32 {
        self.key + *self.value.value(db)
    }
}

#[salsa::input(bare, fields = BareInputFields, debug)]
struct BareInput {
    value: InputField<u32>,
}

impl BareInput {
    fn create(db: &salsa::DatabaseImpl, value: u32) -> Self {
        Input::new(
            db,
            BareInputFields {
                value: InputField::new(db, value),
            },
        )
    }

    fn value(self, db: &dyn salsa::Database) -> u32 {
        *Input::fields(db, self).value.get(db)
    }
}

#[salsa::interned(bare, fields = BareInternedFields, debug)]
struct BareInterned<'db> {
    text: String,
}

impl<'db> BareInterned<'db> {
    fn create(db: &'db dyn salsa::Database, text: impl Into<String>) -> Self {
        Interned::new(db, BareInternedFields { text: text.into() })
    }

    fn text(self, db: &'db dyn salsa::Database) -> &'db str {
        &Interned::fields(db, self).text
    }
}

#[salsa::tracked(bare, fields = BareTrackedFields, debug)]
struct BareTracked<'db> {
    key: u32,
    value: TrackedField<u32>,
}

impl<'db> BareTracked<'db> {
    fn create(db: &'db dyn salsa::Database, key: u32, value: u32) -> Self {
        Tracked::new(
            db,
            BareTrackedFields {
                key,
                value: TrackedField::new(db, value),
            },
        )
    }

    fn value(self, db: &'db dyn salsa::Database) -> u32 {
        *Tracked::fields(db, self).value.value(db)
    }
}

#[salsa::tracked]
fn make_bare_tracked(db: &dyn salsa::Database) -> BareTracked<'_> {
    BareTracked::create(db, 1, 2)
}

#[salsa::tracked]
fn make_macro_tracked(db: &dyn salsa::Database, input: MacroInput) -> MacroTracked<'_> {
    MacroTracked::new(db, input.value(db), input.value(db) + 1)
}

#[derive(Hash)]
struct NameKey<'a> {
    text: &'a str,
    namespace: u32,
}

static NAME_KEY_CONVERSIONS: AtomicUsize = AtomicUsize::new(0);

impl Lookup<NameData> for NameKey<'_> {
    fn into_owned(self) -> NameData {
        NAME_KEY_CONVERSIONS.fetch_add(1, Ordering::Relaxed);
        NameData {
            text: self.text.to_owned(),
            namespace: self.namespace,
        }
    }
}

impl HashEqLike<NameKey<'_>> for NameData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&self.text, state);
        Hash::hash(&self.namespace, state);
    }

    fn eq(&self, key: &NameKey<'_>) -> bool {
        self.text == key.text && self.namespace == key.namespace
    }
}

static DEBUG_RUNS: AtomicUsize = AtomicUsize::new(0);
static NODE_KIND_RUNS: AtomicUsize = AtomicUsize::new(0);
static NODE_TY_RUNS: AtomicUsize = AtomicUsize::new(0);
static NODE_SPAN_RUNS: AtomicUsize = AtomicUsize::new(0);

#[salsa::tracked]
fn read_debug(db: &dyn salsa::Database, options: OptionsHandle) -> bool {
    DEBUG_RUNS.fetch_add(1, Ordering::Relaxed);
    *Input::fields(db, options).debug.get(db)
}

#[salsa::tracked]
fn read_wrapped_debug(db: &dyn salsa::Database, options: OptionsHandle) -> bool {
    *options.data(db).debug.get(db)
}

#[salsa::tracked]
fn make_node<'db>(db: &'db dyn salsa::Database, options: OptionsHandle) -> NodeHandle<'db> {
    let source = *Input::fields(db, options).source.get(db);
    let other_source = *Input::fields(db, options).other_source.get(db);
    Tracked::new(
        db,
        Node {
            kind: 22,
            ty: TrackedField::new(db, source),
            span: TrackedField::new(db, other_source),
        },
    )
}

#[salsa::tracked]
fn make_node_handle<'db>(db: &'db dyn salsa::Database, options: OptionsHandle) -> NodeHandle<'db> {
    make_node(db, options)
}

#[salsa::tracked]
fn read_node_handle(db: &dyn salsa::Database, node: NodeHandle<'_>) -> u32 {
    node.data(db).kind
}

#[salsa::tracked]
fn make_non_clone_node(db: &dyn salsa::Database) -> NonCloneNodeHandle<'_> {
    Tracked::new(
        db,
        NonCloneNode {
            value: TrackedField::new(db, NonCloneValue(22)),
        },
    )
}

#[salsa::tracked]
fn read_node_kind(db: &dyn salsa::Database, options: OptionsHandle) -> u32 {
    NODE_KIND_RUNS.fetch_add(1, Ordering::Relaxed);
    Tracked::fields(db, make_node(db, options)).kind
}

#[salsa::tracked]
fn read_node_ty(db: &dyn salsa::Database, options: OptionsHandle) -> u32 {
    NODE_TY_RUNS.fetch_add(1, Ordering::Relaxed);
    *Tracked::fields(db, make_node(db, options)).ty.value(db)
}

#[salsa::tracked]
fn read_node_span(db: &dyn salsa::Database, options: OptionsHandle) -> u32 {
    NODE_SPAN_RUNS.fetch_add(1, Ordering::Relaxed);
    *Tracked::fields(db, make_node(db, options)).span.value(db)
}

#[salsa::tracked]
fn read_generic_interned(db: &dyn salsa::Database, record: GenericInterned<'_>) -> u32 {
    match record {
        GenericInterned::Name(name) => Interned::fields(db, name).namespace,
        GenericInterned::Symbol(symbol) => Interned::fields(db, symbol).0,
    }
}

#[salsa::tracked]
fn read_name(db: &dyn salsa::Database, name: Name<'_>) -> u32 {
    name.data(db).namespace
}

#[salsa::tracked]
fn make_scoped_node<'db>(db: &'db dyn salsa::Database, options: OptionsHandle) -> ScopedNode<'db> {
    let name = Interned::new(
        db,
        NameData {
            text: "main".to_owned(),
            namespace: 0,
        },
    );
    let ty = *Input::fields(db, options).source.get(db);
    Tracked::new(
        db,
        ScopedNodeData {
            name,
            ty: TrackedField::new(db, ty),
        },
    )
}

fn options(db: &salsa::DatabaseImpl) -> OptionsHandle {
    Input::new(
        db,
        Options {
            debug: InputField::new(db, false),
            target: InputField::new(db, "native".to_owned()),
            source: InputField::new(db, 1),
            other_source: InputField::new(db, 10),
            custom: InputField::new(db, NonCloneValue(0)),
        },
    )
}

#[test]
fn input_fields_invalidate_independently() {
    DEBUG_RUNS.store(0, Ordering::Relaxed);
    let mut db = salsa::DatabaseImpl::default();
    let options = options(&db);

    assert!(!read_debug(&db, options));
    assert_eq!(DEBUG_RUNS.load(Ordering::Relaxed), 1);

    let target = Input::fields(&db, options).target;
    target
        .set(&mut db)
        .with_durability(salsa::Durability::HIGH)
        .to("wasm".to_owned());

    assert!(!read_debug(&db, options));
    assert_eq!(DEBUG_RUNS.load(Ordering::Relaxed), 1);

    let debug = Input::fields(&db, options).debug;
    debug.set(&mut db).to(true);

    assert!(read_debug(&db, options));
    assert_eq!(DEBUG_RUNS.load(Ordering::Relaxed), 2);
}

#[test]
fn input_fields_can_be_modified_in_place() {
    let mut db = salsa::DatabaseImpl::default();
    let options = options(&db);
    let target = Input::fields(&db, options).target;

    target.modify(&mut db, |target| target.push_str("-debug"));

    assert_eq!(target.get(&db), "native-debug");
}

#[test]
fn tracked_fields_preserve_parent_identity_and_invalidate_independently() {
    NODE_KIND_RUNS.store(0, Ordering::Relaxed);
    NODE_TY_RUNS.store(0, Ordering::Relaxed);
    NODE_SPAN_RUNS.store(0, Ordering::Relaxed);
    let mut db = salsa::DatabaseImpl::default();
    let options = options(&db);

    assert_eq!(read_node_kind(&db, options), 22);
    assert_eq!(read_node_ty(&db, options), 1);
    assert_eq!(read_node_span(&db, options), 10);
    assert_eq!(NODE_KIND_RUNS.load(Ordering::Relaxed), 1);
    assert_eq!(NODE_TY_RUNS.load(Ordering::Relaxed), 1);
    assert_eq!(NODE_SPAN_RUNS.load(Ordering::Relaxed), 1);

    let source = Input::fields(&db, options).source;
    source.set(&mut db).to(2);

    assert_eq!(read_node_kind(&db, options), 22);
    assert_eq!(read_node_ty(&db, options), 2);
    assert_eq!(read_node_span(&db, options), 10);
    assert_eq!(NODE_KIND_RUNS.load(Ordering::Relaxed), 1);
    assert_eq!(NODE_TY_RUNS.load(Ordering::Relaxed), 2);
    assert_eq!(NODE_SPAN_RUNS.load(Ordering::Relaxed), 1);
}

#[test]
fn tracked_fields_do_not_require_clone() {
    let db = salsa::DatabaseImpl::default();
    let node = make_non_clone_node(&db);

    assert_eq!(
        Tracked::fields(&db, node).value.value(&db),
        &NonCloneValue(22)
    );
}

#[test]
fn tracked_field_ordinary_traits_delegate_to_the_value() {
    fn hash(value: &impl Hash) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    let db = salsa::DatabaseImpl::default();
    let first = TrackedField::new(&db, 1_u32);
    let second = TrackedField::new(&db, 2_u32);

    assert_ne!(first, second);
    assert_ne!(hash(&first), hash(&second));
}

#[test]
fn interned_lookup_allocates_only_on_a_miss() {
    NAME_KEY_CONVERSIONS.store(0, Ordering::Relaxed);
    let db = salsa::DatabaseImpl::default();

    let first = Interned::<NameData>::new(
        &db,
        NameKey {
            text: "main",
            namespace: 0,
        },
    );
    let second = Interned::<NameData>::new(
        &db,
        NameKey {
            text: "main",
            namespace: 0,
        },
    );

    assert_eq!(first, second);
    assert_eq!(Interned::fields(&db, first).text, "main");
    assert_eq!(NAME_KEY_CONVERSIONS.load(Ordering::Relaxed), 1);
}

#[test]
fn interned_standard_borrowed_lookup() {
    let db = salsa::DatabaseImpl::default();
    let key = TextKey("main".to_owned());
    let first: TextHandle<'_> = Interned::<Text>::new_borrowed(&db, &key);
    let second: TextHandle<'_> = Interned::<Text>::new_borrowed(&db, &key);

    assert_eq!(first, second);
    assert_eq!(Interned::fields(&db, first).0.0, "main");
}

#[test]
fn generic_interned_handles_support_supertypes() {
    let db = salsa::DatabaseImpl::default();
    let name = Interned::new(
        &db,
        NameData {
            text: "main".to_owned(),
            namespace: 22,
        },
    );
    let symbol = Interned::new(&db, SymbolData(44));

    assert_eq!(read_generic_interned(&db, GenericInterned::Name(name)), 22);
    assert_eq!(
        read_generic_interned(&db, GenericInterned::Symbol(symbol)),
        44
    );
}

#[test]
fn struct_wrapper_controls_its_methods() {
    let db = salsa::DatabaseImpl::default();
    let name = Name::intern(&db, "main", 22);

    assert_eq!(name.data(&db).text, "main");
    assert_eq!(read_name(&db, name), 22);
}

#[test]
fn struct_wrapper_delegates_all_salsa_struct_kinds() {
    let db = salsa::DatabaseImpl::default();
    let options = OptionsHandle::create(&db);
    let node = make_node_handle(&db, options);

    assert!(!read_wrapped_debug(&db, options));
    assert_eq!(read_node_handle(&db, node), 22);
}

#[test]
fn lifetime_bearing_data() {
    let db = salsa::DatabaseImpl::default();
    let options = options(&db);
    let namespace = Interned::new(
        &db,
        NameData {
            text: "root".to_owned(),
            namespace: 0,
        },
    );
    let name: ScopedName<'_> = Interned::<ScopedNameData<'_>>::new(
        &db,
        ScopedNameData {
            namespace,
            text: "main".to_owned(),
        },
    );
    let node = make_scoped_node(&db, options);

    assert_eq!(Interned::fields(&db, name).namespace, namespace);
    assert_eq!(Interned::fields(&db, name).text, "main");
    assert_eq!(
        Interned::fields(&db, Tracked::fields(&db, node).name).text,
        "main"
    );
    assert_eq!(*Tracked::fields(&db, node).ty.value(&db), 1);

    #[cfg(feature = "salsa_unstable")]
    {
        let usage = <dyn salsa::Database>::memory_usage(&db);
        let name_info = usage
            .structs
            .iter()
            .find(|info| info.debug_name() == "ScopedNameData<'_>")
            .unwrap();
        assert_eq!(
            name_info.heap_size_of_fields(),
            if cfg!(feature = "get-size") {
                Some("main".len())
            } else {
                None
            }
        );

        let node_info = usage
            .structs
            .iter()
            .find(|info| info.debug_name() == "ScopedNodeData<'_>")
            .unwrap();
        assert_eq!(
            node_info.heap_size_of_fields(),
            if cfg!(feature = "get-size") {
                Some(0)
            } else {
                None
            }
        );
    }
}

#[test]
fn macros_are_facades_over_generic_structs() {
    fn input_raw(_: Input<MacroInputFields>) {}
    fn interned_raw(_: Interned<'_, MacroInternedFields>) {}
    fn tracked_raw(_: Tracked<'_, MacroTrackedFields>) {}

    let db = salsa::DatabaseImpl::default();
    let input = MacroInput::new(&db, 1);
    let interned = MacroInterned::new(&db, "main");
    let tracked = make_macro_tracked(&db, input);

    input_raw(input.0);
    interned_raw(interned.0);
    tracked_raw(tracked.0);

    assert_eq!(input.value(&db), 1);
    assert_eq!(interned.text(&db), "main");
    assert_eq!(tracked.key(&db), 1);
    assert_eq!(tracked.value(&db), 2);
    assert_eq!(input.fields(&db).doubled(), 2);
    assert_eq!(interned.fields(&db).len(), 4);
    assert_eq!(tracked.fields(&db).key_plus_value(&db), 3);

    #[cfg(feature = "get-size")]
    {
        use get_size2::GetSize;
        assert_eq!(input.get_heap_size(), 0);
        assert_eq!(interned.get_heap_size(), 0);
        assert_eq!(tracked.get_heap_size(), 0);
    }
}

#[test]
fn bare_macros_expose_only_the_generic_storage_building_blocks() {
    let db = salsa::DatabaseImpl::default();
    let input = BareInput::create(&db, 1);
    let interned = BareInterned::create(&db, "main");
    let tracked = make_bare_tracked(&db);

    assert_eq!(input.value(&db), 1);
    assert_eq!(interned.text(&db), "main");
    assert_eq!(Tracked::fields(&db, tracked).key, 1);
    assert_eq!(tracked.value(&db), 2);
    assert!(format!("{input:?}").contains("BareInput"));
    assert!(format!("{interned:?}").contains("BareInterned"));
    assert!(format!("{tracked:?}").contains("BareTracked"));
}

#[test]
fn generic_debug_delegates_to_data() {
    let db = salsa::DatabaseImpl::default();
    let name = Interned::new(
        &db,
        NameData {
            text: "main".to_owned(),
            namespace: 0,
        },
    );

    let name = name.0;
    let attached = salsa::attach(&db, || format!("{name:?}"));
    let detached = format!("{name:?}");
    let id = format!("{:?}", salsa::plumbing::AsId::as_id(&name));
    assert_eq!(
        attached,
        format!("Interned({id}, NameData {{ text: \"main\", namespace: 0 }})")
    );
    assert_eq!(detached, format!("Interned(NameData({id}))"));

    let node = make_node(&db, options(&db));
    let node = node.0;
    let attached = salsa::attach(&db, || format!("{node:?}"));
    let detached = format!("{node:?}");
    let id = format!("{:?}", salsa::plumbing::AsId::as_id(&node));
    assert_eq!(
        attached,
        format!("Tracked({id}, Node {{ kind: 22, ty: 1, span: 10 }})")
    );
    assert_eq!(detached, format!("Tracked(Node({id}))"));
}

#[cfg(feature = "get-size")]
#[test]
fn generic_handles_implement_get_size_and_storage_uses_data_size() {
    use get_size2::GetSize;

    let db = salsa::DatabaseImpl::default();
    let options = options(&db);
    let name = Interned::new(
        &db,
        NameData {
            text: "main".to_owned(),
            namespace: 0,
        },
    );
    let node = make_node(&db, options);

    assert_eq!(name.get_heap_size(), 0);
    assert_eq!(node.get_heap_size(), 0);
    assert_eq!(options.get_heap_size(), 0);

    #[cfg(feature = "salsa_unstable")]
    {
        let usage = <dyn salsa::Database>::memory_usage(&db);
        let name_info = usage
            .structs
            .iter()
            .find(|info| info.debug_name() == "NameData")
            .unwrap();
        assert_eq!(
            name_info.heap_size_of_fields(),
            Some(Interned::fields(&db, name).text.capacity())
        );
    }
}
