/// Macro for setting up a function that must intern its arguments.
#[macro_export]
macro_rules! setup_tracked_struct {
    (
        // Attributes on the function.
        attrs: [$(#[$attr:meta]),*],

        // Visibility of the struct.
        vis: $vis:vis,

        // Name of the struct.
        Struct: $Struct:ident,

        // Name and concrete types of the struct containing all fields.
        Fields: $Fields:ident,
        FieldsType: $FieldsType:ty,
        FieldsStaticType: $FieldsStaticType:ty,
        FieldsImplGenerics: [$($FieldsImplLifetime:lifetime)?],
        FieldsRebindLifetime: $FieldsRebindLifetime:lifetime,
        FieldsRebindType: $FieldsRebindType:ty,

        // Name of the batch field accessor (`salsa_fields` if `fields` is occupied).
        fields_fn: $fields_fn:ident,

        // Name of the `'db` lifetime that the user gave.
        db_lt: $db_lt:lifetime,

        // Name user gave for `new`.
        new_fn: $new_fn:ident,

        // Field names.
        field_ids: [$($field_id:ident),*],

        // Tracked field names.
        tracked_ids: [$($tracked_id:ident),*],

        // Untracked field names.
        untracked_ids: [$($untracked_id:ident),*],

        // Whether each field is tracked or untracked.
        field_kinds: [$($field_kind:ident),*],

        // Visibility and names of tracked fields.
        tracked_getters: [$($tracked_getter_vis:vis $tracked_getter_id:ident),*],

        // Visibility and names of untracked fields.
        untracked_getters: [$($untracked_getter_vis:vis $untracked_getter_id:ident),*],

        // Field types, may reference `db_lt`.
        field_tys: [$($field_ty:ty),*],

        // Tracked field types.
        tracked_tys: [$($tracked_ty:ty),*],

        // Untracked field types.
        untracked_tys: [$($untracked_ty:ty),*],

        // Tracked field types.
        tracked_maybe_updates: [$($tracked_maybe_update:tt),*],

        // Untracked field types.
        untracked_maybe_updates: [$($untracked_maybe_update:tt),*],

        // A set of "field options" for each tracked field.
        //
        // Each field option is a tuple `(return_mode, maybe_default)` where:
        //
        // * `return_mode` is an identifier as specified in `salsa_macros::options::Option::returns`
        // * `maybe_default` is either the identifier `default` or `required`
        //
        // These are used to drive conditional logic for each field via recursive macro invocation
        // (see e.g. @return_mode below).
        tracked_options: [$($tracked_option:tt),*],

        // A set of "field options" for each untracked field.
        // (see docs for `tracked_options`).
        untracked_options: [$($untracked_option:tt),*],

        // Attrs for each field.
        tracked_field_attrs: [$([$(#[$tracked_field_attr:meta]),*]),*],
        untracked_field_attrs: [$([$(#[$untracked_field_attr:meta]),*]),*],

        // If true, generate a debug impl.
        generate_debug_impl: $generate_debug_impl:tt,

        // If true, generate constructors, getters, and aggregate accessors.
        generate_methods: $generate_methods:tt,

        // The function used to implement `C::heap_size`.
        heap_size_fn: $($heap_size_fn:path)?,

        // If `true`, `serialize_fn` and `deserialize_fn` have been provided.
        persist: $persist:tt,

        // The path to the `serialize` function for the value's fields.
        serialize_fn: $($serialize_fn:path)?,

        // The path to the `serialize` function for the value's fields.
        deserialize_fn: $($deserialize_fn:path)?,

        // Annoyingly macro-rules hygiene does not extend to items defined in the macro.
        // We have the procedural macro generate names for those items that are
        // not used elsewhere in the user's code.
        unused_names: [
            $zalsa:ident,
            $zalsa_struct:ident,
            $Configuration:ident,
            $CACHE:ident,
            $Db:ident,
            $Revision:ident,
        ]
    ) => {
        $(#[$attr])*
        #[derive(Copy, Clone, PartialEq, Eq, Hash, ::salsa::Struct, ::salsa::Update)]
        #[salsa(configuration, debug = $generate_debug_impl)]
        $vis struct $Struct<$db_lt>(
            ::salsa::Tracked<$db_lt, $FieldsType>
        );

        #[allow(dead_code)]
        #[allow(clippy::all)]
        const _: () = {
            use ::salsa::plumbing as $zalsa;
            use $zalsa::tracked_struct as $zalsa_struct;
            use $zalsa::Revision as $Revision;

            type $Configuration = $Struct<'static>;

            unsafe impl<$($FieldsImplLifetime)?> $zalsa::Update for $FieldsType {
                type Erased = $FieldsStaticType;
                type Rebind<$FieldsRebindLifetime> = $FieldsRebindType;

                unsafe fn maybe_update(old_fields: *mut Self, new_fields: Self) -> bool {
                    use $zalsa::UpdateFallback as _;
                    unsafe {
                        $(
                            $zalsa::TrackedField::maybe_update(
                                std::ptr::addr_of_mut!((*old_fields).$tracked_id),
                                new_fields.$tracked_id,
                                $tracked_maybe_update,
                            ) |
                        )*
                        $(
                            $untracked_maybe_update(
                                std::ptr::addr_of_mut!((*old_fields).$untracked_id),
                                new_fields.$untracked_id,
                            ) |
                        )*
                        false
                    }
                }
            }

            impl $zalsa_struct::Configuration for $Configuration {
                const LOCATION: $zalsa::Location = $zalsa::Location {
                    file: file!(),
                    line: line!(),
                };
                const DEBUG_NAME: &'static str = stringify!($Struct);

                const TRACKED_FIELD_NAMES: &'static [&'static str] =
                    <$FieldsStaticType as $zalsa::generic::TrackedData>::TRACKED_FIELD_NAMES;

                const PERSIST: bool = $persist;

                type Fields<$db_lt> = $FieldsType;

                type Revisions = <$FieldsStaticType as $zalsa::generic::TrackedData>::Revisions;

                type Struct<$db_lt> = $Struct<$db_lt>;

                fn ingredient(zalsa: &$zalsa::Zalsa) -> &$zalsa_struct::IngredientImpl<Self> {
                    Self::ingredient_(zalsa)
                }

                fn bind_tracked_fields(
                    ingredient_index: $zalsa::IngredientIndex,
                    id: $zalsa::Id,
                    fields: &mut Self::Fields<'_>,
                ) {
                    <$FieldsStaticType as $zalsa::generic::TrackedData>::bind_tracked_fields(
                        ingredient_index,
                        id,
                        fields,
                    )
                }

                fn untracked_fields(fields: &Self::Fields<'_>) -> impl ::std::hash::Hash {
                    <$FieldsStaticType as $zalsa::generic::TrackedData>::identity_fields(fields)
                }

                fn new_revisions(current_revision: $Revision) -> Self::Revisions {
                    <$FieldsStaticType as $zalsa::generic::TrackedData>::new_revisions(
                        current_revision,
                    )
                }

                unsafe fn update_fields<$db_lt>(
                    current_revision: $Revision,
                    revisions: &Self::Revisions,
                    old_fields: *mut Self::Fields<$db_lt>,
                    new_fields: Self::Fields<$db_lt>,
                ) -> bool {
                    unsafe {
                        <$FieldsStaticType as $zalsa::generic::TrackedData>::update_fields(
                            current_revision,
                            revisions,
                            old_fields,
                            new_fields,
                        )
                    }
                }

                $(
                    fn heap_size(value: &Self::Fields<'_>) -> Option<usize> {
                        Some($heap_size_fn(value))
                    }
                )?

                fn serialize<S: $zalsa::serde::Serializer>(
                    fields: &Self::Fields<'_>,
                    serializer: S,
                ) -> ::std::result::Result<S::Ok, S::Error> {
                    $zalsa::macro_if! {
                        if $persist {
                            $($serialize_fn(fields, serializer))?
                        } else {
                            panic!("attempted to serialize value not marked with `persist` attribute")
                        }
                    }
                }

                fn deserialize<'de, D: $zalsa::serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> ::std::result::Result<Self::Fields<'static>, D::Error> {
                    $zalsa::macro_if! {
                        if $persist {
                            $($deserialize_fn(deserializer))?
                        } else {
                            panic!("attempted to deserialize value not marked with `persist` attribute")
                        }
                    }
                }
            }

            impl $Configuration {
                $zalsa::macro_if! { $generate_methods =>
                    pub fn ingredient(db: &dyn $zalsa::Database) -> &$zalsa_struct::IngredientImpl<Self> {
                        Self::ingredient_(db.zalsa())
                    }
                }

                fn ingredient_(zalsa: &$zalsa::Zalsa) -> &$zalsa_struct::IngredientImpl<Self> {
                    static CACHE: $zalsa::IngredientCache<$zalsa_struct::IngredientImpl<$Configuration>> =
                        $zalsa::IngredientCache::new();

                    // SAFETY: `lookup_jar_by_type` returns a valid ingredient index, and the only
                    // ingredient created by our jar is the struct ingredient.
                    unsafe {
                        CACHE.get_or_create::<$zalsa_struct::JarImpl<$Configuration>, 0>(zalsa)
                    }
                }
            }

            $zalsa::macro_if! { $persist =>
                impl $zalsa::serde::Serialize for $Struct<'_> {
                    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                    where
                        S: $zalsa::serde::Serializer,
                    {
                        $zalsa::serde::Serialize::serialize(&$zalsa::AsId::as_id(self), serializer)
                    }
                }

                impl<'de> $zalsa::serde::Deserialize<'de> for $Struct<'_> {
                    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
                    where
                        D: $zalsa::serde::Deserializer<'de>,
                    {
                        let id = $zalsa::Id::deserialize(deserializer)?;
                        Ok($zalsa::FromId::from_id(id))
                    }
                }
            }
            $zalsa::macro_if! { $generate_methods =>
            impl<$db_lt> $Struct<$db_lt> {
                pub fn $new_fn<$Db>(db: &$db_lt $Db, $($field_id: $field_ty),*) -> Self
                where
                    // FIXME(rust-lang/rust#65991): The `db` argument *should* have the type `dyn Database`
                    $Db: ?Sized + $zalsa::Database,
                {
                    ::salsa::Tracked::<$FieldsType>::new(
                        db,
                        $Fields {
                            $($field_id: $crate::setup_tracked_struct!(@field_init $field_kind, db, $field_id),)*
                        }
                    )
                }

                $(
                    $(#[$tracked_field_attr])*
                    $tracked_getter_vis fn $tracked_getter_id<$Db>(self, db: &$db_lt $Db) -> $crate::return_mode_ty!($tracked_option, $db_lt, $tracked_ty)
                    where
                        // FIXME(rust-lang/rust#65991): The `db` argument *should* have the type `dyn Database`
                        $Db: ?Sized + $zalsa::Database,
                {
                        let fields = ::salsa::Tracked::<$FieldsType>::fields(db, self);
                        $crate::return_mode_expression!(
                            $tracked_option,
                            $tracked_ty,
                            fields.$tracked_id.value(db),
                        )
                    }
                )*

                $(
                    $(#[$untracked_field_attr])*
                    $untracked_getter_vis fn $untracked_getter_id<$Db>(self, db: &$db_lt $Db) -> $crate::return_mode_ty!($untracked_option, $db_lt, $untracked_ty)
                    where
                        // FIXME(rust-lang/rust#65991): The `db` argument *should* have the type `dyn Database`
                        $Db: ?Sized + $zalsa::Database,
                {
                        let fields = ::salsa::Tracked::<$FieldsType>::fields(db, self);
                        $crate::return_mode_expression!(
                            $untracked_option,
                            $untracked_ty,
                            &fields.$untracked_id,
                        )
                    }
                )*

                /// Returns all fields without recording tracked-field reads.
                pub fn $fields_fn<$Db>(self, db: &$db_lt $Db) -> &$db_lt $FieldsType
                where
                    $Db: ?Sized + $zalsa::Database,
                {
                    ::salsa::Tracked::<$FieldsType>::fields(db, self)
                }
            }

            #[allow(unused_lifetimes)]
            impl<'_db> $Struct<'_db> {
                /// Default debug formatting for this struct (may be useful if you define your own `Debug` impl)
                pub fn default_debug_fmt(this: Self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result
                where
                    $(for<$db_lt> $field_ty: ::std::fmt::Debug),*
                {
                    ::std::fmt::Debug::fmt($zalsa::Struct::as_repr(&this), f)
                }
            }
            }
        };
    };

    (@field_init tracked, $db:expr, $value:expr) => {
        ::salsa::TrackedField::new($db, $value)
    };

    (@field_init untracked, $db:expr, $value:expr) => {
        $value
    };
}
