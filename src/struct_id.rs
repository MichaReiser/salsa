//! Nominal wrappers around generic Salsa struct handles.

use std::any::TypeId;

use crate::id::{AsId, FromId};
use crate::memo_ingredient_indices::IngredientIndices;
use crate::salsa_struct::SalsaStructInDb;
use crate::table::memo::MemoTableWithTypes;
use crate::tracked_struct::TrackedStructInDb;
use crate::zalsa::{HasJar, JarKind, Zalsa};
use crate::{DatabaseKeyIndex, Id, Revision, Update};

/// Capability passed by Salsa when reconstructing a nominal struct wrapper.
#[doc(hidden)]
pub struct StructToken {
    _private: (),
}

impl StructToken {
    fn new() -> Self {
        Self { _private: () }
    }
}

/// Plumbing implemented by the built-in generic Salsa struct handles.
#[doc(hidden)]
pub trait StructRepr: AsId + FromId + HasJar + SalsaStructInDb {}

impl<T> StructRepr for T where T: AsId + FromId + HasJar + SalsaStructInDb {}

/// Extracts the stored data type from an interned representation.
#[doc(hidden)]
pub trait InternedStructRepr {
    type Data;
}

impl<T> InternedStructRepr for crate::Interned<'_, T>
where
    T: Update,
{
    type Data = T;
}

/// A nominal wrapper around a generic Salsa struct handle.
///
/// Deriving this trait does not generate constructors, field accessors, or
/// ordinary trait implementations. It only teaches Salsa how to move between
/// the wrapper and its generic representation, leaving the wrapper's public
/// API under user control.
///
/// ```
/// #[cfg_attr(feature = "get-size", derive(get_size2::GetSize))]
/// #[derive(Clone, PartialEq, Eq, Hash, salsa::InternedData, salsa::Update)]
/// struct NameData(String);
///
/// #[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Struct, salsa::Update)]
/// struct Name<'db>(salsa::Interned<'db, NameData>);
/// ```
///
/// ```compile_fail
/// #[derive(salsa::Struct)]
/// struct NotASalsaStruct(u32);
/// ```
pub trait Struct: Sized + Update {
    /// The generic [`crate::Tracked`], [`crate::Interned`], or [`crate::Input`]
    /// handle wrapped by this type.
    type Repr: StructRepr;

    /// Wraps the generic representation.
    #[doc(hidden)]
    fn from_repr(token: StructToken, repr: Self::Repr) -> Self;

    /// Borrows the generic representation.
    #[doc(hidden)]
    fn as_repr(&self) -> &Self::Repr;
}

impl<T> AsId for T
where
    T: Struct,
    T::Repr: AsId,
{
    fn as_id(&self) -> Id {
        self.as_repr().as_id()
    }
}

impl<T> FromId for T
where
    T: Struct,
    T::Repr: FromId,
{
    fn from_id(id: Id) -> Self {
        Self::from_repr(StructToken::new(), T::Repr::from_id(id))
    }
}

impl<T> HasJar for T
where
    T: Struct,
    T::Repr: HasJar,
{
    type Jar = <T::Repr as HasJar>::Jar;
    const KIND: JarKind = <T::Repr as HasJar>::KIND;
}

impl<T> SalsaStructInDb for T
where
    T: Struct,
    T::Repr: SalsaStructInDb,
{
    type MemoIngredientMap = <T::Repr as SalsaStructInDb>::MemoIngredientMap;
    const LEAF_TYPE_IDS: &'static [typeid::ConstTypeId] = &[typeid::ConstTypeId::of::<T::Erased>()];

    fn lookup_ingredient_index(zalsa: &Zalsa) -> IngredientIndices {
        T::Repr::lookup_ingredient_index(zalsa)
    }

    fn entries(zalsa: &Zalsa) -> impl Iterator<Item = DatabaseKeyIndex> + '_ {
        T::Repr::entries(zalsa)
    }

    fn cast(id: Id, type_id: TypeId) -> Option<Self> {
        (type_id == TypeId::of::<T::Erased>()).then(|| Self::from_id(id))
    }

    unsafe fn memo_table(
        zalsa: &Zalsa,
        id: Id,
        current_revision: Revision,
    ) -> MemoTableWithTypes<'_> {
        // SAFETY: the caller guarantees that `id` identifies a live value of
        // this Salsa struct. `Struct` uses the same storage as `Repr`.
        unsafe { T::Repr::memo_table(zalsa, id, current_revision) }
    }
}

impl<T> TrackedStructInDb for T
where
    T: Struct,
    T::Repr: TrackedStructInDb,
{
    fn database_key_index(zalsa: &Zalsa, id: Id) -> DatabaseKeyIndex {
        T::Repr::database_key_index(zalsa, id)
    }
}
