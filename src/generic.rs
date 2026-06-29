//! Generic, macro-free Salsa struct handles.

use std::any::TypeId;
use std::borrow::{Borrow, ToOwned};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use crate::id::{AsId, FromId};
use crate::ingredient::{Ingredient, Location};
use crate::ingredient_cache::IngredientIndexCache;
use crate::input::singleton::NotSingleton;
use crate::memo_ingredient_indices::{IngredientIndices, MemoIngredientSingletonIndex};
use crate::revision::AtomicRevision;
use crate::salsa_struct::SalsaStructInDb;
use crate::table::memo::MemoTableWithTypes;
use crate::tracked_struct::TrackedStructInDb;
use crate::zalsa::{HasJar, JarKind, Zalsa};
use crate::{
    Database, DatabaseKeyIndex, Durability, Id, Revision, Setter, Update, with_attached_database,
};

/// Common bounds and heap accounting for stored data.
#[doc(hidden)]
pub trait StoredData: Send + Sync {
    fn heap_size(&self) -> Option<usize>;
}

/// Declares data stored behind a generic [`Tracked`] handle.
///
/// Direct [`TrackedField`] members receive independent dependency revisions.
/// Nested tracked fields are rejected because they do not have a stable field
/// position in the containing data.
///
/// ```
/// #[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
/// #[derive(Hash, salsa::TrackedData, salsa::Update)]
/// struct Expression {
///     kind: u32,
///     ty: salsa::TrackedField<u32>,
/// }
///
/// #[derive(Copy, Clone, Eq, Hash, PartialEq, salsa::Struct, salsa::Update)]
/// struct ExpressionId<'db>(salsa::Tracked<'db, Expression>);
/// ```
///
/// ```compile_fail
/// #[derive(Hash, salsa::TrackedData, salsa::Update)]
/// struct Invalid {
///     nested: Vec<salsa::TrackedField<u32>>,
/// }
/// ```
pub trait TrackedData: 'static {
    #[cfg(feature = "persistence")]
    type Revisions: Send
        + Sync
        + std::ops::Index<usize, Output = AtomicRevision>
        + crate::plumbing::serde::Serialize
        + for<'de> crate::plumbing::serde::Deserialize<'de>;

    #[cfg(not(feature = "persistence"))]
    type Revisions: Send + Sync + std::ops::Index<usize, Output = AtomicRevision>;

    const TRACKED_FIELD_NAMES: &'static [&'static str];
    const INGREDIENT_CACHE: &'static IngredientIndexCache;

    fn bind_tracked_fields(
        ingredient_index: crate::IngredientIndex,
        id: Id,
        fields: &mut <Self as Update>::Rebind<'_>,
    ) where
        Self: Update;

    fn identity_fields(fields: &<Self as Update>::Rebind<'_>) -> impl Hash
    where
        Self: Update;

    fn new_revisions(current_revision: Revision) -> Self::Revisions;

    /// Updates stored data and the revisions of independently tracked fields.
    ///
    /// # Safety
    ///
    /// The arguments must satisfy [`Update::maybe_update`]'s contract.
    unsafe fn update_fields<'db>(
        current_revision: Revision,
        revisions: &Self::Revisions,
        old_fields: *mut <Self as Update>::Rebind<'db>,
        new_fields: <Self as Update>::Rebind<'db>,
    ) -> bool
    where
        Self: Update;
}

/// Declares data stored behind a generic [`Interned`] handle.
///
/// ```
/// #[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
/// #[derive(Clone, PartialEq, Eq, Hash, salsa::InternedData, salsa::Update)]
/// struct NameData {
///     text: String,
/// }
///
/// #[derive(Copy, Clone, Eq, Hash, PartialEq, salsa::Struct, salsa::Update)]
/// struct Name<'db>(salsa::Interned<'db, NameData>);
/// ```
///
/// ```compile_fail
/// #[derive(Clone, PartialEq, Eq, Hash, salsa::InternedData, salsa::Update)]
/// struct Invalid {
///     tracked: salsa::TrackedField<u32>,
/// }
/// ```
pub trait InternedData: 'static {
    const INGREDIENT_CACHE: &'static IngredientIndexCache;
}

/// Declares data stored behind a generic [`Input`] handle.
///
/// ```
/// #[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
/// #[derive(salsa::InputData)]
/// struct Options {
///     debug: salsa::InputField<bool>,
/// }
///
/// #[derive(Copy, Clone, Eq, Hash, PartialEq, salsa::Struct, salsa::Update)]
/// struct OptionsId(salsa::Input<Options>);
/// ```
///
/// ```compile_fail
/// #[derive(salsa::InputData)]
/// struct Invalid {
///     tracked: salsa::TrackedField<u32>,
/// }
/// ```
pub trait InputData: 'static {
    const INGREDIENT_CACHE: &'static IngredientIndexCache;
}

/// Connects tracked data to its nominal Salsa struct and storage configuration.
#[doc(hidden)]
pub trait TrackedDataConfig: TrackedData {
    type Configuration: crate::tracked_struct::Configuration;
}

/// Connects interned data to its nominal Salsa struct and storage configuration.
#[doc(hidden)]
pub trait InternedDataConfig: InternedData {
    type Configuration: crate::interned::Configuration;
}

/// Connects input data to its nominal Salsa struct and storage configuration.
#[doc(hidden)]
pub trait InputDataConfig: InputData {
    type Configuration: crate::input::Configuration<
            Fields = Self,
            Revisions = [Revision; 1],
            Durabilities = [Durability; 1],
        >;
}

#[cfg(not(feature = "get-size"))]
impl<T> StoredData for T
where
    T: Send + Sync,
{
    fn heap_size(&self) -> Option<usize> {
        None
    }
}

#[cfg(feature = "get-size")]
impl<T> StoredData for T
where
    T: get_size2::GetSize + Send + Sync,
{
    fn heap_size(&self) -> Option<usize> {
        Some(self.get_heap_size())
    }
}

/// Adapts an erased [`Update`] lifetime family to a configurable tracked struct.
#[doc(hidden)]
pub struct TrackedConfig<T, S>(PhantomData<fn() -> (T, S)>);

impl<T, S> crate::tracked_struct::Configuration for TrackedConfig<T, S>
where
    T: StoredData + Hash + TrackedData + Update<Erased = T>,
    for<'db> T::Rebind<'db>: StoredData + Hash + Update<Erased = T, Rebind<'db> = T::Rebind<'db>>,
    S: Update<Erased = S> + 'static,
    for<'db> S::Rebind<'db>: Copy + FromId + AsId,
{
    const LOCATION: Location = Location {
        file: file!(),
        line: line!(),
    };
    const DEBUG_NAME: &'static str = "Tracked";
    const PERSIST: bool = false;

    const TRACKED_FIELD_NAMES: &'static [&'static str] = T::TRACKED_FIELD_NAMES;

    type Fields<'db> = T::Rebind<'db>;
    type Struct<'db> = S::Rebind<'db>;
    type Revisions = T::Revisions;

    fn debug_name() -> &'static str {
        short_type_name::<T>()
    }

    fn ingredient(zalsa: &Zalsa) -> &crate::tracked_struct::IngredientImpl<Self> {
        cached_generic_ingredient::<
            crate::tracked_struct::JarImpl<Self>,
            crate::tracked_struct::IngredientImpl<Self>,
        >(
            zalsa,
            T::INGREDIENT_CACHE,
            "tracked",
            Self::debug_name(),
            "`#[derive(salsa::TrackedData)]` on its data type",
        )
    }

    fn bind_tracked_fields(
        ingredient_index: crate::IngredientIndex,
        id: Id,
        fields: &mut Self::Fields<'_>,
    ) {
        T::bind_tracked_fields(ingredient_index, id, fields);
    }

    fn untracked_fields(fields: &Self::Fields<'_>) -> impl Hash {
        T::identity_fields(fields)
    }

    fn new_revisions(current_revision: Revision) -> Self::Revisions {
        T::new_revisions(current_revision)
    }

    unsafe fn update_fields<'db>(
        current_revision: Revision,
        revisions: &Self::Revisions,
        old_fields: *mut Self::Fields<'db>,
        new_fields: Self::Fields<'db>,
    ) -> bool {
        // SAFETY: forwarded from the configuration contract.
        unsafe { T::update_fields(current_revision, revisions, old_fields, new_fields) }
    }

    fn heap_size(value: &Self::Fields<'_>) -> Option<usize> {
        value.heap_size()
    }

    fn serialize<Serializer>(
        _: &Self::Fields<'_>,
        _: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: crate::plumbing::serde::Serializer,
    {
        panic!("generic tracked data are not persistable")
    }

    fn deserialize<'de, D>(_: D) -> Result<Self::Fields<'static>, D::Error>
    where
        D: crate::plumbing::serde::Deserializer<'de>,
    {
        panic!("generic tracked data are not persistable")
    }
}

/// A lifetime-branded ID for a tracked data.
///
/// With an attached database, its [`Debug`](fmt::Debug) output contains the
/// ID followed by `T`'s debug output. Without one, it contains the ID only.
/// The tuple name is the final path component of
/// [`std::any::type_name::<T>()`](std::any::type_name).
///
/// With the `get-size` feature, this implements `get_size2::GetSize` when `T`
/// does. The handle owns no data heap memory; Salsa accounts for `T` in the
/// ingredient that stores it.
///
/// The brand prevents mutating the database while the handle remains live:
///
/// ```compile_fail
/// use salsa::Database as _;
///
/// #[derive(Hash, salsa::TrackedData, salsa::Update)]
/// struct Data(u32);
///
/// #[derive(Copy, Clone, Eq, Hash, PartialEq, salsa::Struct, salsa::Update)]
/// struct DataId<'db>(salsa::Tracked<'db, Data>);
///
/// #[salsa::tracked]
/// fn make(db: &dyn salsa::Database) -> DataId<'_> {
///     salsa::Tracked::new(db, Data(0))
/// }
///
/// let mut db = salsa::DatabaseImpl::default();
/// let value = make(&db);
/// db.synthetic_write(salsa::Durability::LOW);
/// let _ = salsa::Tracked::fields(&db, value);
/// ```
#[repr(transparent)]
pub struct Tracked<'db, T>
where
    T: Update,
{
    id: Id,
    phantom: PhantomData<fn() -> (&'db (), T)>,
}

type TrackedConfigFor<T> = <<T as Update>::Erased as TrackedDataConfig>::Configuration;

impl<'db, T> Tracked<'db, T>
where
    T: Update,
    T::Erased: TrackedDataConfig,
    TrackedConfigFor<T>: crate::tracked_struct::Configuration<Fields<'db> = T>,
{
    /// Creates a tracked data in the active query.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<Db>(
        db: &'db Db,
        fields: T,
    ) -> <TrackedConfigFor<T> as crate::tracked_struct::Configuration>::Struct<'db>
    where
        Db: ?Sized + Database,
    {
        let (zalsa, zalsa_local) = db.zalsas();
        tracked_ingredient::<TrackedConfigFor<T>>(zalsa).new_struct(zalsa, zalsa_local, fields)
    }

    /// Returns the tracked data.
    pub fn fields<Db, S>(db: &'db Db, value: S) -> &'db T
    where
        Db: ?Sized + Database,
        S: crate::Struct<Repr = Tracked<'db, T>>,
        TrackedConfigFor<T>: crate::tracked_struct::Configuration<Struct<'db> = S>,
    {
        let zalsa = db.zalsa();
        tracked_ingredient::<TrackedConfigFor<T>>(zalsa).untracked_field(zalsa, value)
    }
}

/// A value stored in, and tracked through, its parent tracked data.
///
/// The wrapper is intentionally neither `Copy` nor `Clone`. Its projection is
/// bound by the parent's [`crate::tracked_struct::Configuration`] when the
/// parent receives an ID.
pub struct TrackedField<T> {
    projection: Option<DatabaseKeyIndex>,
    value: T,
}

impl<T> TrackedField<T> {
    /// Creates an unbound tracked field for data under construction.
    pub fn new<Db>(_db: &Db, value: T) -> Self
    where
        Db: ?Sized + Database,
    {
        Self {
            projection: None,
            value,
        }
    }

    /// Returns the current value.
    ///
    /// This records a dependency on this particular field rather than on the
    /// parent record.
    pub fn value<'db, Db>(&'db self, db: &'db Db) -> &'db T
    where
        Db: ?Sized + Database,
    {
        let (zalsa, zalsa_local) = db.zalsas();
        let projection = self
            .projection
            .expect("tracked field was read before its parent record was created");
        let stamp = zalsa
            .lookup_ingredient(projection.ingredient_index())
            .dependency_stamp(zalsa, projection.key_index());
        zalsa_local.report_tracked_read_simple(projection, stamp.durability, stamp.changed_at);
        &self.value
    }

    /// Binds this field to an existing parent projection ingredient.
    #[doc(hidden)]
    pub fn bind(
        &mut self,
        parent_ingredient: crate::IngredientIndex,
        parent_id: Id,
        relative_field_index: usize,
    ) {
        self.projection = Some(DatabaseKeyIndex::new(
            parent_ingredient.successor(relative_field_index),
            parent_id,
        ));
    }

    /// Updates the value while preserving its parent projection identity.
    ///
    /// # Safety
    ///
    /// `old_pointer` and `maybe_update` must satisfy the corresponding
    /// [`Update::maybe_update`] contract.
    #[doc(hidden)]
    pub unsafe fn maybe_update(
        old_pointer: *mut Self,
        new_value: Self,
        maybe_update: unsafe fn(*mut T, T) -> bool,
    ) -> bool {
        // SAFETY: forwarded from the tracked configuration's update contract.
        unsafe {
            maybe_update(
                std::ptr::addr_of_mut!((*old_pointer).value),
                new_value.value,
            )
        }
    }
}

impl<T> fmt::Debug for TrackedField<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<T> PartialEq for TrackedField<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> Eq for TrackedField<T> where T: Eq {}

impl<T> Hash for TrackedField<T>
where
    T: Hash,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

#[cfg(feature = "persistence")]
impl<T> serde::Serialize for TrackedField<T>
where
    T: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.value.serialize(serializer)
    }
}

#[cfg(feature = "persistence")]
impl<'de, T> serde::Deserialize<'de> for TrackedField<T>
where
    T: serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self {
            projection: None,
            value: T::deserialize(deserializer)?,
        })
    }
}

// SAFETY: the projection is database-owned metadata and remains bound to the
// old parent while the value follows `T`'s lifetime family.
unsafe impl<T> Update for TrackedField<T>
where
    T: Update,
    T::Erased: Update,
    for<'db> T::Rebind<'db>: Update,
{
    type Erased = TrackedField<T::Erased>;
    type Rebind<'db> = TrackedField<T::Rebind<'db>>;

    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        // SAFETY: guaranteed by the `Update` contract. The new unbound
        // projection is intentionally discarded.
        unsafe {
            T::maybe_update(
                std::ptr::addr_of_mut!((*old_pointer).value),
                new_value.value,
            )
        }
    }
}

/// Adapts an erased [`Update`] lifetime family to a configurable interned struct.
#[doc(hidden)]
pub struct InternedConfig<T, S>(PhantomData<fn() -> (T, S)>);

impl<T, S> crate::interned::Configuration for InternedConfig<T, S>
where
    T: StoredData + InternedData + Update<Erased = T>,
    for<'db> T::Rebind<'db>: StoredData
        + crate::interned::InternedValue
        + Update<Erased = T, Rebind<'db> = T::Rebind<'db>>,
    S: Update<Erased = S> + 'static,
    for<'db> S::Rebind<'db>: Copy + FromId + AsId,
{
    const LOCATION: Location = Location {
        file: file!(),
        line: line!(),
    };
    const DEBUG_NAME: &'static str = "Interned";
    const PERSIST: bool = false;

    type Fields<'db> = T::Rebind<'db>;
    type Struct<'db> = S::Rebind<'db>;

    fn debug_name() -> &'static str {
        short_type_name::<T>()
    }

    fn ingredient(zalsa: &Zalsa) -> &crate::interned::IngredientImpl<Self> {
        cached_generic_ingredient::<
            crate::interned::JarImpl<Self>,
            crate::interned::IngredientImpl<Self>,
        >(
            zalsa,
            T::INGREDIENT_CACHE,
            "interned",
            Self::debug_name(),
            "`#[derive(salsa::InternedData)]` on its data type",
        )
    }

    fn heap_size(value: &Self::Fields<'_>) -> Option<usize> {
        value.heap_size()
    }

    fn serialize<Serializer>(
        _: &Self::Fields<'_>,
        _: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: crate::plumbing::serde::Serializer,
    {
        panic!("generic interned data are not persistable")
    }

    fn deserialize<'de, D>(_: D) -> Result<Self::Fields<'static>, D::Error>
    where
        D: crate::plumbing::serde::Deserializer<'de>,
    {
        panic!("generic interned data are not persistable")
    }
}

/// A lifetime-branded ID for an interned data.
///
/// Debug formatting and optional `get_size2::GetSize` integration follow the
/// same rules as [`Tracked`].
///
/// References and handles retain the database borrow that protects interned
/// storage from mutation:
///
/// ```compile_fail
/// use salsa::Database as _;
///
/// #[derive(Clone, Eq, Hash, PartialEq, salsa::InternedData, salsa::Update)]
/// struct Text(String);
///
/// #[derive(Copy, Clone, Eq, Hash, PartialEq, salsa::Struct, salsa::Update)]
/// struct TextId<'db>(salsa::Interned<'db, Text>);
///
/// let mut db = salsa::DatabaseImpl::default();
/// let text = salsa::Interned::new(&db, Text("main".to_owned()));
/// db.synthetic_write(salsa::Durability::LOW);
/// let _ = salsa::Interned::fields(&db, text);
/// ```
///
/// Record kinds are checked by their declaration trait:
///
/// ```compile_fail,E0277
/// #[derive(Clone, Eq, Hash, PartialEq, salsa::TrackedData, salsa::Update)]
/// struct Data(u32);
///
/// let db = salsa::DatabaseImpl::default();
/// let _ = salsa::Interned::<Data>::new(&db, Data(0));
/// ```
#[repr(transparent)]
pub struct Interned<'db, T>
where
    T: Update,
{
    id: Id,
    phantom: PhantomData<fn() -> (&'db (), T)>,
}

// These static functions use the `'static` specialization only as a namespace.
// Their own `'db` parameter still brands the configured struct they return or read;
// a `no_lifetime` configuration deliberately maps that GAT back to one static wrapper.
impl<T> Interned<'static, T>
where
    T: Update,
    T::Erased: InternedDataConfig,
{
    /// Interns a whole-value lookup key.
    ///
    /// The key is converted to owned data only when interning misses.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<'db, Db, K>(
        db: &'db Db,
        key: K,
    ) -> <InternedConfigFor<T> as crate::interned::Configuration>::Struct<'db>
    where
        Db: ?Sized + Database,
        K: Hash + crate::Lookup<T>,
        T: crate::HashEqLike<K>,
        InternedConfigFor<T>: crate::interned::Configuration<Fields<'db> = T>,
    {
        let (zalsa, zalsa_local) = db.zalsas();
        let id = interned_ingredient::<InternedConfigFor<T>>(zalsa).intern_id(
            zalsa,
            zalsa_local,
            key,
            |_, key| key.into_owned(),
        );
        FromId::from_id(id)
    }

    /// Interns a standard borrowed key without allocating on a hit.
    pub fn new_borrowed<'db, Db, Q>(
        db: &'db Db,
        key: &Q,
    ) -> <InternedConfigFor<T> as crate::interned::Configuration>::Struct<'db>
    where
        Db: ?Sized + Database,
        T: Borrow<Q>,
        Q: ?Sized + Eq + Hash + ToOwned<Owned = T>,
        InternedConfigFor<T>: crate::interned::Configuration<Fields<'db> = T>,
    {
        let (zalsa, zalsa_local) = db.zalsas();
        FromId::from_id(
            interned_ingredient::<InternedConfigFor<T>>(zalsa).intern_id(
                zalsa,
                zalsa_local,
                BorrowedKey(key),
                |_, key| key.0.to_owned(),
            ),
        )
    }

    /// Returns the interned data.
    pub fn fields<'db, Db, S>(db: &'db Db, value: S) -> &'db T
    where
        Db: ?Sized + Database,
        S: crate::Struct,
        S::Repr: crate::InternedStructRepr<Data = T>,
        InternedConfigFor<T>: crate::interned::Configuration<Fields<'db> = T, Struct<'db> = S>,
    {
        let zalsa = db.zalsa();
        interned_ingredient::<InternedConfigFor<T>>(zalsa).data(zalsa, value.as_id())
    }
}

type InternedConfigFor<T> = <<T as Update>::Erased as InternedDataConfig>::Configuration;

/// Configuration for a one-field ordinary input data.
#[doc(hidden)]
pub struct InputConfig<T, S>(PhantomData<fn() -> (T, S)>);

/// An input containing an ordinary data struct.
///
/// Debug formatting and optional `get_size2::GetSize` integration follow the
/// same rules as [`Tracked`], except this handle is not lifetime branded.
#[repr(transparent)]
pub struct Input<T> {
    id: Id,
    phantom: PhantomData<fn() -> T>,
}

type InputConfigFor<T> = <T as InputDataConfig>::Configuration;

impl<T> Input<T>
where
    T: InputDataConfig,
    InputConfigFor<T>: crate::input::Configuration<Fields = T>,
{
    /// Creates a new input data.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<Db>(db: &Db, fields: T) -> <InputConfigFor<T> as crate::input::Configuration>::Struct
    where
        Db: ?Sized + Database,
        InputConfigFor<T>: crate::input::Configuration<Singleton = NotSingleton>,
    {
        new_input::<Db, InputConfigFor<T>>(db, fields)
    }

    /// Reads the immutable input record without recording a dependency.
    ///
    /// Mutable values are stored behind explicit [`InputField`] handles,
    /// which record their own dependencies when read.
    pub fn fields<Db, S>(db: &Db, value: S) -> &T
    where
        Db: ?Sized + Database,
        S: crate::Struct<Repr = Input<T>>,
        InputConfigFor<T>: crate::input::Configuration<Struct = S>,
    {
        let zalsa = db.zalsa();
        <InputConfigFor<T> as crate::input::Configuration>::ingredient(zalsa)
            .leak_fields(zalsa, value)
    }

    /// Reads the whole value without recording a dependency.
    #[doc(hidden)]
    pub fn fields_untracked<Db, S>(db: &Db, value: S) -> &T
    where
        Db: ?Sized + Database,
        S: crate::Struct<Repr = Input<T>>,
        InputConfigFor<T>: crate::input::Configuration<Struct = S>,
    {
        let zalsa = db.zalsa();
        <InputConfigFor<T> as crate::input::Configuration>::ingredient(zalsa)
            .leak_fields(zalsa, value)
    }

}

/// An independently stored mutable field referenced by an immutable input record.
#[repr(transparent)]
pub struct InputField<T> {
    id: Id,
    phantom: PhantomData<fn() -> T>,
}

impl<T> InputField<T>
where
    T: StoredData + 'static,
{
    /// Creates a new independently tracked input field.
    pub fn new<Db>(db: &Db, value: T) -> Self
    where
        Db: ?Sized + Database,
    {
        new_input::<Db, InputFieldConfig<T>>(db, value)
    }

    #[doc(hidden)]
    pub fn new_with_durability<Db>(db: &Db, value: T, durability: Durability) -> Self
    where
        Db: ?Sized + Database,
    {
        new_input_with_durability::<Db, InputFieldConfig<T>>(db, value, durability)
    }

    /// Reads this field and tracks only this field.
    pub fn get<Db>(self, db: &Db) -> &T
    where
        Db: ?Sized + Database,
    {
        read_input::<Db, InputFieldConfig<T>>(self, db)
    }

    /// Reads this field without recording a dependency.
    #[doc(hidden)]
    pub fn get_untracked<Db>(self, db: &Db) -> &T
    where
        Db: ?Sized + Database,
    {
        let zalsa = db.zalsa();
        input_ingredient_for_id::<InputFieldConfig<T>>(zalsa, self.as_id())
            .leak_fields(zalsa, self)
    }

    /// Replaces this field's value.
    #[must_use]
    pub fn set<'db, Db>(self, db: &'db mut Db) -> impl Setter<FieldTy = T> + 'db
    where
        Db: ?Sized + Database,
    {
        set_input::<Db, InputFieldConfig<T>>(self, db)
    }

    /// Mutates this field in place and returns the closure's result.
    pub fn modify<Db, R>(self, db: &mut Db, modify: impl FnOnce(&mut T) -> R) -> R
    where
        Db: ?Sized + Database,
    {
        let index = db.zalsa().ingredient_index(self.id);
        let zalsa = db.zalsa_mut();
        zalsa.new_revision();
        let (ingredient, runtime) = zalsa.lookup_ingredient_mut(index);
        let ingredient = ingredient.assert_type_mut::<crate::input::IngredientImpl<
            InputFieldConfig<T>,
        >>();
        ingredient.set_field(runtime, self, 0, None, modify)
    }
}

#[doc(hidden)]
pub struct InputFieldConfig<T>(PhantomData<fn() -> T>);

impl<T> crate::input::Configuration for InputFieldConfig<T>
where
    T: StoredData + 'static,
{
    const DEBUG_NAME: &'static str = "InputField";
    const FIELD_DEBUG_NAMES: &'static [&'static str] = &["value"];
    const LOCATION: Location = Location {
        file: file!(),
        line: line!(),
    };
    const PERSIST: bool = false;

    type Singleton = NotSingleton;
    type Struct = InputField<T>;
    type Fields = T;
    type Revisions = [Revision; 1];
    type Durabilities = [Durability; 1];

    fn debug_name() -> &'static str {
        short_type_name::<T>()
    }

    fn ingredient(zalsa: &Zalsa) -> &crate::input::IngredientImpl<Self> {
        lookup_generic_ingredient::<crate::input::JarImpl<Self>, crate::input::IngredientImpl<Self>>(
            zalsa,
            "input field",
            short_type_name::<T>(),
            "an `InputData` declaration containing `InputField<T>`",
        )
    }

    fn heap_size(value: &Self::Fields) -> Option<usize> {
        value.heap_size()
    }

    fn serialize<S>(_: &Self::Fields, _: S) -> Result<S::Ok, S::Error>
    where
        S: crate::plumbing::serde::Serializer,
    {
        panic!("generic input field values are not persistable")
    }

    fn deserialize<'de, D>(_: D) -> Result<Self::Fields, D::Error>
    where
        D: crate::plumbing::serde::Deserializer<'de>,
    {
        panic!("generic input field values are not persistable")
    }
}

impl<T, S> crate::input::Configuration for InputConfig<T, S>
where
    T: StoredData + InputData,
    S: FromId + AsId + Send + Sync + 'static,
{
    const DEBUG_NAME: &'static str = "Input";
    const FIELD_DEBUG_NAMES: &'static [&'static str] = &["fields"];
    const LOCATION: Location = Location {
        file: file!(),
        line: line!(),
    };
    const PERSIST: bool = false;

    type Singleton = NotSingleton;
    type Struct = S;
    type Fields = T;
    type Revisions = [Revision; 1];
    type Durabilities = [Durability; 1];

    fn debug_name() -> &'static str {
        short_type_name::<T>()
    }

    fn ingredient(zalsa: &Zalsa) -> &crate::input::IngredientImpl<Self> {
        cached_generic_ingredient::<crate::input::JarImpl<Self>, crate::input::IngredientImpl<Self>>(
            zalsa,
            T::INGREDIENT_CACHE,
            "input",
            short_type_name::<T>(),
            "`#[derive(salsa::InputData)]` on its data type",
        )
    }

    fn heap_size(value: &Self::Fields) -> Option<usize> {
        value.heap_size()
    }

    fn serialize<Serializer>(
        _: &Self::Fields,
        _: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: crate::plumbing::serde::Serializer,
    {
        panic!("generic input values are not persistable")
    }

    fn deserialize<'de, D>(_: D) -> Result<Self::Fields, D::Error>
    where
        D: crate::plumbing::serde::Deserializer<'de>,
    {
        panic!("generic input values are not persistable")
    }
}

fn tracked_ingredient<C>(zalsa: &Zalsa) -> &crate::tracked_struct::IngredientImpl<C>
where
    C: crate::tracked_struct::Configuration,
{
    C::ingredient(zalsa)
}

fn interned_ingredient<C>(zalsa: &Zalsa) -> &crate::interned::IngredientImpl<C>
where
    C: crate::interned::Configuration,
{
    C::ingredient(zalsa)
}

#[track_caller]
fn cached_generic_ingredient<'db, J, I>(
    zalsa: &'db Zalsa,
    cache: &IngredientIndexCache,
    kind: &str,
    name: &str,
    declaration: &str,
) -> &'db I
where
    J: crate::ingredient::Jar,
    I: Ingredient,
{
    // SAFETY: each declaration gives every Salsa struct kind a distinct cache
    // slot, and each generic jar contains one ingredient at offset zero.
    let index = unsafe { cache.try_get_or_create::<J, 0>(zalsa) }
        .unwrap_or_else(|| generic_ingredient_missing(kind, name, declaration));

    // SAFETY: the cache slot can only be initialized from `J`, whose ingredient
    // at offset zero has type `I`.
    unsafe {
        zalsa
            .lookup_ingredient_unchecked(index)
            .assert_type_unchecked::<I>()
    }
}

#[track_caller]
fn lookup_generic_ingredient<'db, J, I>(
    zalsa: &'db Zalsa,
    kind: &str,
    name: &str,
    declaration: &str,
) -> &'db I
where
    J: crate::ingredient::Jar,
    I: Ingredient,
{
    let index = zalsa
        .try_lookup_jar_by_type::<J>()
        .unwrap_or_else(|| generic_ingredient_missing(kind, name, declaration));
    zalsa.lookup_ingredient(index).assert_type()
}

#[track_caller]
fn generic_ingredient_missing(kind: &str, name: &str, declaration: &str) -> ! {
    panic!(
        "Salsa {kind} data `{name}` is not registered in this database. \
         Declare {declaration} and enable inventory, or register its handle with the \
         database builder. A declaration for a different Salsa struct kind does not apply."
    )
}

fn short_type_name<T: ?Sized>() -> &'static str {
    last_path_component(std::any::type_name::<T>())
}

fn last_path_component(name: &'static str) -> &'static str {
    let bytes = name.as_bytes();
    let mut depth = 0_u32;
    let mut start = 0;
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' | b')' | b']' => depth = depth.saturating_sub(1),
            b':' if depth == 0 && bytes.get(index + 1) == Some(&b':') => {
                start = index + 2;
                index += 1;
            }
            _ => {}
        }
        index += 1;
    }

    &name[start..]
}

fn new_input<Db, C>(db: &Db, value: C::Fields) -> C::Struct
where
    Db: ?Sized + Database,
    C: crate::input::Configuration<
            Revisions = [Revision; 1],
            Durabilities = [Durability; 1],
            Singleton = NotSingleton,
        >,
{
    new_input_with_durability::<Db, C>(db, value, Durability::default())
}

fn new_input_with_durability<Db, C>(
    db: &Db,
    value: C::Fields,
    durability: Durability,
) -> C::Struct
where
    Db: ?Sized + Database,
    C: crate::input::Configuration<
            Revisions = [Revision; 1],
            Durabilities = [Durability; 1],
            Singleton = NotSingleton,
        >,
{
    let (zalsa, zalsa_local) = db.zalsas();
    let revision = zalsa.current_revision();
    C::ingredient(zalsa).new_input(zalsa, zalsa_local, value, [revision], [durability])
}

fn read_input<Db, C>(value: C::Struct, db: &Db) -> &C::Fields
where
    Db: ?Sized + Database,
    C: crate::input::Configuration,
{
    let (zalsa, zalsa_local) = db.zalsas();
    input_ingredient_for_id::<C>(zalsa, value.as_id()).field(zalsa, zalsa_local, value, 0)
}

fn set_input<'db, Db, C>(
    value: C::Struct,
    db: &'db mut Db,
) -> impl Setter<FieldTy = C::Fields> + 'db
where
    Db: ?Sized + Database,
    C: crate::input::Configuration,
{
    let index = db.zalsa().ingredient_index(value.as_id());
    let zalsa = db.zalsa_mut();
    zalsa.new_revision();
    let (ingredient, runtime) = zalsa.lookup_ingredient_mut(index);
    let ingredient = ingredient.assert_type_mut::<crate::input::IngredientImpl<C>>();
    crate::input::setter::SetterImpl::new(runtime, value, 0, ingredient, |field, value| {
        std::mem::replace(field, value)
    })
}

fn input_ingredient_for_id<C>(zalsa: &Zalsa, id: Id) -> &crate::input::IngredientImpl<C>
where
    C: crate::input::Configuration,
{
    // SAFETY: `id` was created by this configured input ingredient.
    unsafe {
        zalsa
            .lookup_ingredient_unchecked(zalsa.ingredient_index(id))
            .assert_type_unchecked()
    }
}

#[derive(Hash)]
struct BorrowedKey<'a, Q: ?Sized>(&'a Q);

impl<Q, T> crate::HashEqLike<BorrowedKey<'_, Q>> for T
where
    Q: ?Sized + Eq + Hash,
    T: Borrow<Q>,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.borrow().hash(state);
    }

    fn eq(&self, data: &BorrowedKey<'_, Q>) -> bool {
        self.borrow() == data.0
    }
}

macro_rules! impl_data_handle {
    ($handle:ident) => {
        impl<T: Update> Copy for $handle<'_, T> {}

        impl<T: Update> Clone for $handle<'_, T> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<T: Update> PartialEq for $handle<'_, T> {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }

        impl<T: Update> Eq for $handle<'_, T> {}

        impl<T: Update> PartialOrd for $handle<'_, T> {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl<T: Update> Ord for $handle<'_, T> {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.id.cmp(&other.id)
            }
        }

        impl<T: Update> Hash for $handle<'_, T> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.id.hash(state);
            }
        }

        impl<T: Update> AsId for $handle<'_, T> {
            fn as_id(&self) -> Id {
                self.id
            }
        }

        impl<T: Update> FromId for $handle<'_, T> {
            fn from_id(id: Id) -> Self {
                Self {
                    id,
                    phantom: PhantomData,
                }
            }
        }

        // SAFETY: ID handles have no recursively updateable state, and their
        // lifetime family follows the data's `Update` family.
        unsafe impl<T> Update for $handle<'_, T>
        where
            T: Update,
            T::Erased: Update,
            for<'db> T::Rebind<'db>: Update,
        {
            type Erased = $handle<'static, T::Erased>;
            type Rebind<'db> = $handle<'db, T::Rebind<'db>>;

            unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
                // SAFETY: guaranteed by the `Update` contract.
                let old_value = unsafe { &mut *old_pointer };
                if *old_value == new_value {
                    false
                } else {
                    *old_value = new_value;
                    true
                }
            }
        }
    };
}

impl_data_handle!(Tracked);
impl_data_handle!(Interned);

macro_rules! impl_id_handle {
    ($handle:ident<$($parameter:ident),+>) => {
        impl<$($parameter),+> Copy for $handle<$($parameter),+> {}

        impl<$($parameter),+> Clone for $handle<$($parameter),+> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<$($parameter),+> PartialEq for $handle<$($parameter),+> {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }

        impl<$($parameter),+> Eq for $handle<$($parameter),+> {}

        impl<$($parameter),+> PartialOrd for $handle<$($parameter),+> {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl<$($parameter),+> Ord for $handle<$($parameter),+> {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.id.cmp(&other.id)
            }
        }

        impl<$($parameter),+> Hash for $handle<$($parameter),+> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.id.hash(state);
            }
        }

        impl<$($parameter),+> AsId for $handle<$($parameter),+> {
            fn as_id(&self) -> Id {
                self.id
            }
        }

        impl<$($parameter),+> FromId for $handle<$($parameter),+> {
            fn from_id(id: Id) -> Self {
                Self {
                    id,
                    phantom: PhantomData,
                }
            }
        }

        // SAFETY: ID handles have no recursively updateable state.
        unsafe impl<$($parameter: 'static),+> Update for $handle<$($parameter),+> {
            type Erased = Self;
            type Rebind<'db> = Self;

            unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
                // SAFETY: guaranteed by the `Update` contract.
                let old_value = unsafe { &mut *old_pointer };
                if *old_value == new_value {
                    false
                } else {
                    *old_value = new_value;
                    true
                }
            }
        }
    };
}

impl_id_handle!(InputField<T>);
impl_id_handle!(Input<T>);

impl<T> fmt::Debug for Tracked<'_, T>
where
    T: Update,
    T::Erased: TrackedDataConfig,
    TrackedConfigFor<T>: crate::tracked_struct::Configuration,
    for<'db> <TrackedConfigFor<T> as crate::tracked_struct::Configuration>::Fields<'db>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        type Config<T> = TrackedConfigFor<T>;

        with_attached_database(|db| {
            let zalsa = db.zalsa();
            let fields =
                tracked_ingredient::<Config<T>>(zalsa).leak_fields(zalsa, FromId::from_id(self.id));
            f.debug_tuple("Tracked")
                .field(&self.id)
                .field(fields)
                .finish()
        })
        .unwrap_or_else(|| {
            f.debug_tuple("Tracked")
                .field(&DebugNamedId {
                    name: <Config<T> as crate::tracked_struct::Configuration>::debug_name(),
                    id: self.id,
                })
                .finish()
        })
    }
}

impl<T> fmt::Debug for Interned<'_, T>
where
    T: Update,
    T::Erased: InternedDataConfig,
    InternedConfigFor<T>: crate::interned::Configuration,
    for<'db> <InternedConfigFor<T> as crate::interned::Configuration>::Fields<'db>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        type Config<T> = InternedConfigFor<T>;

        with_attached_database(|db| {
            let zalsa = db.zalsa();
            let fields = interned_ingredient::<Config<T>>(zalsa).data(zalsa, self.id);
            f.debug_tuple("Interned")
                .field(&self.id)
                .field(fields)
                .finish()
        })
        .unwrap_or_else(|| {
            f.debug_tuple("Interned")
                .field(&DebugNamedId {
                    name: <Config<T> as crate::interned::Configuration>::debug_name(),
                    id: self.id,
                })
                .finish()
        })
    }
}

struct DebugNamedId {
    name: &'static str,
    id: Id,
}

impl fmt::Debug for DebugNamedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(self.name).field(&self.id).finish()
    }
}

impl<T> fmt::Debug for InputField<T>
where
    T: fmt::Debug + StoredData + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_attached_database(|db| {
            f.debug_tuple("InputField")
                .field(&self.id)
                .field(self.get_untracked(db))
                .finish()
        })
        .unwrap_or_else(|| {
            f.debug_tuple("InputField")
                .field(&DebugNamedId {
                    name: short_type_name::<T>(),
                    id: self.id,
                })
                .finish()
        })
    }
}

#[cfg(feature = "persistence")]
impl<T> serde::Serialize for InputField<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.id.serialize(serializer)
    }
}

#[cfg(feature = "persistence")]
impl<'de, T> serde::Deserialize<'de> for InputField<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(FromId::from_id(Id::deserialize(deserializer)?))
    }
}

impl<T> fmt::Debug for Input<T>
where
    T: fmt::Debug + InputDataConfig,
    InputConfigFor<T>: crate::input::Configuration<Fields = T>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        type Config<T> = InputConfigFor<T>;

        with_attached_database(|db| {
            let zalsa = db.zalsa();
            let value: <Config<T> as crate::input::Configuration>::Struct =
                FromId::from_id(self.id);
            let fields = <Config<T> as crate::input::Configuration>::ingredient(zalsa)
                .leak_fields(zalsa, value);
            f.debug_tuple("Input")
                .field(&self.id)
                .field(fields)
                .finish()
        })
        .unwrap_or_else(|| {
            f.debug_tuple("Input")
                .field(&DebugNamedId {
                    name: <Config<T> as crate::input::Configuration>::debug_name(),
                    id: self.id,
                })
                .finish()
        })
    }
}

#[cfg(feature = "get-size")]
impl<T> get_size2::GetSize for Tracked<'_, T> where T: get_size2::GetSize + Update {}

#[cfg(feature = "get-size")]
impl<T> get_size2::GetSize for Interned<'_, T> where T: get_size2::GetSize + Update {}

#[cfg(feature = "get-size")]
impl<T> get_size2::GetSize for InputField<T> {}

#[cfg(feature = "get-size")]
impl<T> get_size2::GetSize for Input<T> {}

#[cfg(feature = "get-size")]
impl<T> get_size2::GetSize for TrackedField<T>
where
    T: get_size2::GetSize,
{
    fn get_heap_size_with_tracker<S>(&self, tracker: S) -> (usize, S)
    where
        S: get_size2::GetSizeTracker,
    {
        self.value.get_heap_size_with_tracker(tracker)
    }
}

impl<'db, T> HasJar for Tracked<'db, T>
where
    T: Update,
    T::Erased: TrackedDataConfig,
    TrackedConfigFor<T>: crate::tracked_struct::Configuration,
{
    type Jar = crate::tracked_struct::JarImpl<TrackedConfigFor<T>>;
    const KIND: JarKind = JarKind::Struct;
}

impl<'db, T> HasJar for Interned<'db, T>
where
    T: Update,
    T::Erased: InternedDataConfig,
    InternedConfigFor<T>: crate::interned::Configuration,
{
    type Jar = crate::interned::JarImpl<InternedConfigFor<T>>;
    const KIND: JarKind = JarKind::Struct;
}

impl<T> HasJar for InputField<T>
where
    T: StoredData + 'static,
{
    type Jar = crate::input::JarImpl<InputFieldConfig<T>>;
    const KIND: JarKind = JarKind::Struct;
}

impl<T> HasJar for Input<T>
where
    T: InputDataConfig,
    InputConfigFor<T>: crate::input::Configuration,
{
    type Jar = crate::input::JarImpl<InputConfigFor<T>>;
    const KIND: JarKind = JarKind::Struct;
}

// The concrete implementations are written out because their jar types depend
// on generic configuration parameters and cannot be passed through a `path`
// macro fragment followed by type arguments.
impl<T> SalsaStructInDb for Tracked<'_, T>
where
    T: Update,
    T::Erased: Update + TrackedDataConfig,
    TrackedConfigFor<T>: crate::tracked_struct::Configuration,
{
    type MemoIngredientMap = MemoIngredientSingletonIndex;
    const LEAF_TYPE_IDS: &'static [typeid::ConstTypeId] =
        &[typeid::ConstTypeId::of::<Tracked<'static, T::Erased>>()];

    fn lookup_ingredient_index(zalsa: &Zalsa) -> IngredientIndices {
        zalsa
            .lookup_jar_by_type::<crate::tracked_struct::JarImpl<TrackedConfigFor<T>>>()
            .into()
    }

    fn entries(zalsa: &Zalsa) -> impl Iterator<Item = DatabaseKeyIndex> + '_ {
        tracked_ingredient::<TrackedConfigFor<T>>(zalsa)
            .entries(zalsa)
            .map(|entry| entry.key())
    }

    fn cast(id: Id, type_id: TypeId) -> Option<Self> {
        (type_id == TypeId::of::<Tracked<'static, T::Erased>>()).then(|| FromId::from_id(id))
    }

    unsafe fn memo_table(
        zalsa: &Zalsa,
        id: Id,
        current_revision: Revision,
    ) -> MemoTableWithTypes<'_> {
        // SAFETY: guaranteed by the caller.
        unsafe {
            zalsa
                .table()
                .memos::<crate::tracked_struct::Value<TrackedConfigFor<T>>>(id, current_revision)
        }
    }
}

impl<T> TrackedStructInDb for Tracked<'_, T>
where
    T: Update,
    T::Erased: Update + TrackedDataConfig,
    TrackedConfigFor<T>: crate::tracked_struct::Configuration,
{
    fn database_key_index(zalsa: &Zalsa, id: Id) -> DatabaseKeyIndex {
        tracked_ingredient::<TrackedConfigFor<T>>(zalsa).database_key_index(id)
    }
}

impl<T> SalsaStructInDb for Interned<'_, T>
where
    T: Update,
    T::Erased: Update + InternedDataConfig,
    InternedConfigFor<T>: crate::interned::Configuration,
{
    type MemoIngredientMap = MemoIngredientSingletonIndex;
    const LEAF_TYPE_IDS: &'static [typeid::ConstTypeId] =
        &[typeid::ConstTypeId::of::<Interned<'static, T::Erased>>()];

    fn lookup_ingredient_index(zalsa: &Zalsa) -> IngredientIndices {
        zalsa
            .lookup_jar_by_type::<crate::interned::JarImpl<InternedConfigFor<T>>>()
            .into()
    }

    fn entries(zalsa: &Zalsa) -> impl Iterator<Item = DatabaseKeyIndex> + '_ {
        interned_ingredient::<InternedConfigFor<T>>(zalsa)
            .entries(zalsa)
            .map(|entry| entry.key())
    }

    fn cast(id: Id, type_id: TypeId) -> Option<Self> {
        (type_id == TypeId::of::<Interned<'static, T::Erased>>()).then(|| FromId::from_id(id))
    }

    unsafe fn memo_table(
        zalsa: &Zalsa,
        id: Id,
        current_revision: Revision,
    ) -> MemoTableWithTypes<'_> {
        // SAFETY: guaranteed by the caller.
        unsafe {
            zalsa
                .table()
                .memos::<crate::interned::Value<InternedConfigFor<T>>>(id, current_revision)
        }
    }
}

impl<T> SalsaStructInDb for InputField<T>
where
    T: StoredData + 'static,
{
    type MemoIngredientMap = MemoIngredientSingletonIndex;
    const LEAF_TYPE_IDS: &'static [typeid::ConstTypeId] =
        &[typeid::ConstTypeId::of::<InputField<T>>()];

    fn lookup_ingredient_index(zalsa: &Zalsa) -> IngredientIndices {
        zalsa
            .lookup_jar_by_type::<crate::input::JarImpl<InputFieldConfig<T>>>()
            .into()
    }

    fn entries(zalsa: &Zalsa) -> impl Iterator<Item = DatabaseKeyIndex> + '_ {
        <InputFieldConfig<T> as crate::input::Configuration>::ingredient(zalsa)
            .entries(zalsa)
            .map(|entry| entry.key())
    }

    fn cast(id: Id, type_id: TypeId) -> Option<Self> {
        (type_id == TypeId::of::<InputField<T>>()).then(|| FromId::from_id(id))
    }

    unsafe fn memo_table(
        zalsa: &Zalsa,
        id: Id,
        current_revision: Revision,
    ) -> MemoTableWithTypes<'_> {
        // SAFETY: guaranteed by the caller.
        unsafe {
            zalsa
                .table()
                .memos::<crate::input::Value<InputFieldConfig<T>>>(id, current_revision)
        }
    }
}

impl<T> SalsaStructInDb for Input<T>
where
    T: InputDataConfig,
    InputConfigFor<T>: crate::input::Configuration,
{
    type MemoIngredientMap = MemoIngredientSingletonIndex;
    const LEAF_TYPE_IDS: &'static [typeid::ConstTypeId] = &[typeid::ConstTypeId::of::<Input<T>>()];

    fn lookup_ingredient_index(zalsa: &Zalsa) -> IngredientIndices {
        zalsa
            .lookup_jar_by_type::<crate::input::JarImpl<InputConfigFor<T>>>()
            .into()
    }

    fn entries(zalsa: &Zalsa) -> impl Iterator<Item = DatabaseKeyIndex> + '_ {
        <InputConfigFor<T> as crate::input::Configuration>::ingredient(zalsa)
            .entries(zalsa)
            .map(|entry| entry.key())
    }

    fn cast(id: Id, type_id: TypeId) -> Option<Self> {
        (type_id == TypeId::of::<Input<T>>()).then(|| FromId::from_id(id))
    }

    unsafe fn memo_table(
        zalsa: &Zalsa,
        id: Id,
        current_revision: Revision,
    ) -> MemoTableWithTypes<'_> {
        // SAFETY: guaranteed by the caller.
        unsafe {
            zalsa
                .table()
                .memos::<crate::input::Value<InputConfigFor<T>>>(id, current_revision)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::last_path_component;

    #[test]
    fn debug_names_use_the_last_top_level_path_component() {
        assert_eq!(last_path_component("crate::Record"), "Record");
        assert_eq!(
            last_path_component("crate::Record<other::Value>"),
            "Record<other::Value>"
        );
    }
}
