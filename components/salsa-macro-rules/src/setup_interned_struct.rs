/// Macro for setting up a function that must intern its arguments.
#[macro_export]
macro_rules! setup_interned_struct {
    (
        // Attributes on the struct
        attrs: [$(#[$attr:meta]),*],

        // Visibility of the struct
        vis: $vis:vis,

        // Name of the struct
        Struct: $Struct:ident,

        // Name and concrete types of the struct containing the interned fields.
        Fields: $Fields:ident,
        FieldsType: $FieldsType:ty,
        FieldsStaticType: $FieldsStaticType:ty,
        FieldsImplGenerics: [$($FieldsImplLifetime:lifetime)?],
        FieldsRebindLifetime: $FieldsRebindLifetime:lifetime,
        FieldsRebindType: $FieldsRebindType:ty,

        // Name of the batch field accessor (`salsa_fields` if `fields` is occupied).
        fields_fn: $fields_fn:ident,

        // Name of the struct type with a `'static` argument (unless this type has no db lifetime,
        // in which case this is the same as `$Struct`)
        StructWithStatic: $StructWithStatic:ty,

        // Name of the `'db` lifetime that the user gave
        db_lt: $db_lt:lifetime,

        // optional db lifetime argument.
        db_lt_arg: $($db_lt_arg:lifetime)?,

        // the salsa ID
        id: $Id:path,

        // The minimum number of revisions to keep the value interned.
        revisions: $($revisions:expr)?,

        // the lifetime used in the desugared interned struct.
        // if the `db_lt_arg`, is present, this is `db_lt_arg`, but otherwise,
        // it is `'static`.
        interior_lt: $interior_lt:lifetime,

        // Name user gave for `new`
        new_fn: $new_fn:ident,

        // A series of option tuples; see `setup_tracked_struct` macro
        field_options: [$($field_option:tt),*],

        // Field names
        field_ids: [$($field_id:ident),*],

        // Names for field setter methods (typically `set_foo`)
        field_getters: [$($field_getter_vis:vis $field_getter_id:ident),*],

        // Field types
        field_tys: [$($field_ty:ty),*],

        // Indices for each field from 0..N -- must be unsuffixed (e.g., `0`, `1`).
        field_indices: [$($field_index:tt),*],

        // Indexed types for each field (T0, T1, ...)
        field_indexed_tys: [$($indexed_ty:ident),*],

        // Attrs for each field.
        field_attrs: [$([$(#[$field_attr:meta]),*]),*],

        // If true, generate a debug impl.
        generate_debug_impl: $generate_debug_impl:tt,

        // If true, generate constructors and accessors.
        generate_methods: $generate_methods:tt,

        // The function used to implement `C::heap_size`.
        heap_size_fn: $($heap_size_fn:path)?,

        // If `true`, `serialize_fn` and `deserialize_fn` have been provided.
        persist: $persist:tt,

        // The path to the `serialize` function for the value's fields.
        serialize_fn: $($serialize_fn:path)?,

        // The path to the `serialize` function for the value's fields.
        deserialize_fn: $($deserialize_fn:path)?,

        assert_types_are_update: {$($assert_types_are_update:tt)*},

        // Annoyingly macro-rules hygiene does not extend to items defined in the macro.
        // We have the procedural macro generate names for those items that are
        // not used elsewhere in the user's code.
        unused_names: [
            $zalsa:ident,
            $zalsa_struct:ident,
            $Configuration:ident,
            $CACHE:ident,
            $Db:ident,
        ]
    ) => {
        $(#[$attr])*
        #[derive(Copy, Clone, PartialEq, Eq, Hash, ::salsa::Struct, ::salsa::Update)]
        #[salsa(configuration, debug = $generate_debug_impl)]
        $vis struct $Struct< $($db_lt_arg)? >(
            ::salsa::Interned<$interior_lt, $FieldsType>
        );

        #[allow(clippy::all)]
        #[allow(dead_code)]
        const _: () = {
            use ::salsa::plumbing as $zalsa;
            use $zalsa::interned as $zalsa_struct;

            type $Configuration = $StructWithStatic;

            $($assert_types_are_update)*

            unsafe impl<$($FieldsImplLifetime)?> $zalsa::Update for $FieldsType {
                type Erased = $FieldsStaticType;
                type Rebind<$FieldsRebindLifetime> = $FieldsRebindType;

                unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
                    // Interned fields are immutable after insertion. This implementation
                    // exists to describe the lifetime family used by the representation.
                    unsafe { *old_pointer = new_value };
                    true
                }
            }

            /// Key to use during hash lookups. Each field is some type that implements `Lookup<T>`
            /// for the owned type. This permits interning with an `&str` when a `String` is required and so forth.
            #[derive(Hash)]
            struct StructKey<$db_lt, $($indexed_ty),*>(
                $($indexed_ty,)*
                ::std::marker::PhantomData<&$db_lt ()>,
            );

            impl<$db_lt, $($indexed_ty,)*> $zalsa::HashEqLike<StructKey<$db_lt, $($indexed_ty),*>>
                for $FieldsType
                where
                $($field_ty: $zalsa::HashEqLike<$indexed_ty>),*
                {

                fn hash<H: ::std::hash::Hasher>(&self, h: &mut H) {
                    $($zalsa::HashEqLike::<$indexed_ty>::hash(&self.$field_id, &mut *h);)*
                }

                fn eq(&self, data: &StructKey<$db_lt, $($indexed_ty),*>) -> bool {
                    ($($zalsa::HashEqLike::<$indexed_ty>::eq(&self.$field_id, &data.$field_index) && )* true)
                }
            }

            impl<$db_lt, $($indexed_ty: $zalsa::Lookup<$field_ty>),*> $zalsa::Lookup<$FieldsType>
                for StructKey<$db_lt, $($indexed_ty),*> {

                #[allow(unused_unit)]
                fn into_owned(self) -> $FieldsType {
                    $Fields {
                        $($field_id: $zalsa::Lookup::into_owned(self.$field_index),)*
                    }
                }
            }

            impl<$db_lt> ::std::cmp::PartialEq<($($field_ty,)*)> for $FieldsType {
                fn eq(&self, other: &($($field_ty,)*)) -> bool {
                    $((&self.$field_id == &other.$field_index) &&)* true
                }
            }

            impl $zalsa::interned::Configuration for $StructWithStatic {
                const LOCATION: $zalsa::Location = $zalsa::Location {
                    file: file!(),
                    line: line!(),
                };
                const DEBUG_NAME: &'static str = stringify!($Struct);
                const PERSIST: bool = $persist;

                $(
                    const REVISIONS: ::core::num::NonZeroUsize = ::core::num::NonZeroUsize::new($revisions).unwrap();
                )?

                type Fields<$db_lt> = $FieldsType;
                type Struct<$db_lt> = $Struct<$($db_lt_arg)?>;

                fn ingredient(zalsa: &$zalsa::Zalsa) -> &$zalsa_struct::IngredientImpl<Self> {
                    Self::ingredient_(zalsa)
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
                    pub fn ingredient(zalsa: &$zalsa::Zalsa) -> &$zalsa_struct::IngredientImpl<Self> {
                        Self::ingredient_(zalsa)
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
                impl<$($db_lt_arg)?> $zalsa::serde::Serialize for $Struct<$($db_lt_arg)?> {
                    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                    where
                        S: $zalsa::serde::Serializer,
                    {
                        $zalsa::serde::Serialize::serialize(&$zalsa::AsId::as_id(self), serializer)
                    }
                }

                impl<'de, $($db_lt_arg)?> $zalsa::serde::Deserialize<'de> for $Struct<$($db_lt_arg)?> {
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
            impl<$db_lt> $Struct< $($db_lt_arg)? >  {
                pub fn $new_fn<$Db, $($indexed_ty: $zalsa::Lookup<$field_ty> + ::std::hash::Hash,)*>(db: &$db_lt $Db,  $($field_id: $indexed_ty),*) -> Self
                where
                    // FIXME(rust-lang/rust#65991): The `db` argument *should* have the type `dyn Database`
                    $Db: ?Sized + ::salsa::Database,
                    $(
                        $field_ty: $zalsa::HashEqLike<$indexed_ty>,
                    )*
                {
                    let key =
                        StructKey::<$db_lt>($($field_id,)* ::std::marker::PhantomData::default());
                    let value = ::salsa::Interned::<$FieldsType>::new(db, key);
                    let configured_id: $Id =
                        $zalsa::FromId::from_id($zalsa::AsId::as_id(&value));
                    Self($zalsa::FromId::from_id($zalsa::AsId::as_id(&configured_id)))
                }

                $(
                    $(#[$field_attr])*
                    $field_getter_vis fn $field_getter_id<$Db>(self, db: &'db $Db) -> $zalsa::return_mode_ty!($field_option, 'db, $field_ty)
                    where
                        // FIXME(rust-lang/rust#65991): The `db` argument *should* have the type `dyn Database`
                        $Db: ?Sized + $zalsa::Database,
                {
                        let fields = ::salsa::Interned::<$FieldsType>::fields(db, self);
                        $zalsa::return_mode_expression!(
                            $field_option,
                            $field_ty,
                            &fields.$field_id,
                        )
                    }
                )*

                /// Returns all fields in one storage read.
                pub fn $fields_fn<$Db>(self, db: &$db_lt $Db) -> &$db_lt $FieldsType
                where
                    $Db: ?Sized + $zalsa::Database,
                {
                    ::salsa::Interned::<$FieldsType>::fields(db, self)
                }
            }

            $zalsa::macro_if! {
                iftt ($($db_lt_arg)?) {
                    impl $Struct<'_> {
                        /// Default debug formatting for this struct (may be useful if you define your own `Debug` impl)
                        pub fn default_debug_fmt(this: Self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result
                        where
                            $(for<$db_lt> $field_ty: ::std::fmt::Debug),*
                        {
                            ::std::fmt::Debug::fmt($zalsa::Struct::as_repr(&this), f)
                        }
                    }
                } else {
                    impl $Struct {
                        /// Default debug formatting for this struct (may be useful if you define your own `Debug` impl)
                        pub fn default_debug_fmt(this: Self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result
                        where
                            $(for<$db_lt> $field_ty: ::std::fmt::Debug),*
                        {
                            ::std::fmt::Debug::fmt($zalsa::Struct::as_repr(&this), f)
                        }
                    }
                }
            }
            }
        };
    };
}
