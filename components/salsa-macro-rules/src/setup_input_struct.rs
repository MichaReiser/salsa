/// Sets up an input struct backed by `salsa::Input<Fields>`.
#[macro_export]
macro_rules! setup_input_struct {
    (
        attrs: [$(#[$attr:meta]),*],
        vis: $vis:vis,
        Struct: $Struct:ident,
        Fields: $Fields:ident,
        fields_fn: $fields_fn:ident,
        new_fn: $new_fn:ident,
        field_options: [$($field_option:tt),*],
        field_ids: [$($field_id:ident),*],
        field_getters: [$($field_getter_vis:vis $field_getter_id:ident),*],
        field_setters: [$($field_setter_vis:vis $field_setter_id:ident),*],
        field_tys: [$($field_ty:ty),*],
        field_value_tys: [$($field_value_ty:ty),*],
        field_kinds: [$($field_kind:ident),*],
        field_indices: [$($field_index:tt),*],
        field_attrs: [$([$(#[$field_attr:meta]),*]),*],
        required_fields: [$($required_field_id:ident $required_field_ty:ty),*],
        field_durability_ids: [$($field_durability_id:ident),*],
        num_fields: $N:literal,
        is_singleton: $is_singleton:tt,
        generate_debug_impl: $generate_debug_impl:tt,
        generate_methods: $generate_methods:tt,
        heap_size_fn: $($heap_size_fn:path)?,
        persist: $persist:tt,
        serialize_fn: $($serialize_fn:path)?,
        deserialize_fn: $($deserialize_fn:path)?,
        unused_names: [
            $zalsa:ident,
            $zalsa_struct:ident,
            $Configuration:ident,
            $Builder:ident,
            $CACHE:ident,
            $Db:ident,
        ]
    ) => {
        $(#[$attr])*
        #[derive(Copy, Clone, PartialEq, Eq, Hash, ::salsa::Struct, ::salsa::Update)]
        #[salsa(configuration, debug = $generate_debug_impl)]
        $vis struct $Struct(::salsa::Input<$Fields>);

        #[allow(clippy::all)]
        #[allow(dead_code)]
        const _: () = {
            use ::salsa::plumbing as $zalsa;
            use $zalsa::input as $zalsa_struct;

            type $Configuration = $Struct;

            impl $zalsa_struct::Configuration for $Configuration {
                const LOCATION: $zalsa::Location = $zalsa::Location {
                    file: file!(),
                    line: line!(),
                };
                const DEBUG_NAME: &'static str = stringify!($Struct);
                const FIELD_DEBUG_NAMES: &'static [&'static str] = &["fields"];
                const PERSIST: bool = $persist;

                type Singleton = $zalsa::macro_if! {
                    if $is_singleton {
                        $zalsa::input::Singleton
                    } else {
                        $zalsa::input::NotSingleton
                    }
                };
                type Struct = $Struct;
                type Fields = $Fields;
                type Revisions = [$zalsa::Revision; 1];
                type Durabilities = [$zalsa::Durability; 1];

                fn ingredient(zalsa: &$zalsa::Zalsa) -> &$zalsa_struct::IngredientImpl<Self> {
                    Self::ingredient_(zalsa)
                }

                $(
                    fn heap_size(value: &Self::Fields) -> Option<usize> {
                        Some($heap_size_fn(value))
                    }
                )?

                fn serialize<S: $zalsa::serde::Serializer>(
                    fields: &Self::Fields,
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
                ) -> ::std::result::Result<Self::Fields, D::Error> {
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
                    pub fn ingredient(
                        db: &dyn $zalsa::Database,
                    ) -> &$zalsa_struct::IngredientImpl<Self> {
                        Self::ingredient_(db.zalsa())
                    }
                }

                fn ingredient_(zalsa: &$zalsa::Zalsa) -> &$zalsa_struct::IngredientImpl<Self> {
                    static CACHE: $zalsa::IngredientCache<$zalsa_struct::IngredientImpl<$Configuration>> =
                        $zalsa::IngredientCache::new();

                    // SAFETY: this jar's first and only ingredient is the input struct ingredient.
                    unsafe {
                        CACHE.get_or_create::<$zalsa_struct::JarImpl<$Configuration>, 0>(zalsa)
                    }
                }
            }

            $zalsa::macro_if! { $persist =>
                impl $zalsa::serde::Serialize for $Struct {
                    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                    where
                        S: $zalsa::serde::Serializer,
                    {
                        $zalsa::serde::Serialize::serialize(&$zalsa::AsId::as_id(self), serializer)
                    }
                }

                impl<'de> $zalsa::serde::Deserialize<'de> for $Struct {
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
                impl $Struct {
                    #[inline]
                    pub fn $new_fn<$Db>(db: &$Db, $($required_field_id: $required_field_ty),*) -> Self
                    where
                        $Db: ?Sized + $zalsa::Database,
                    {
                        Self::builder($($required_field_id,)*).new(db)
                    }

                    pub fn builder($($required_field_id: $required_field_ty),*) -> <Self as $zalsa_struct::HasBuilder>::Builder {
                        builder::new_builder($($zalsa::maybe_default!($field_option, $field_value_ty, $field_id,)),*)
                    }

                    $(
                        $(#[$field_attr])*
                        $field_getter_vis fn $field_getter_id<'db, $Db>(
                            self,
                            db: &'db $Db,
                        ) -> $zalsa::return_mode_ty!($field_option, 'db, $field_value_ty)
                        where
                            $Db: ?Sized + $zalsa::Database,
                        {
                            $crate::setup_input_struct!(
                                @get $field_kind,
                                $Configuration,
                                $Fields,
                                db,
                                self,
                                $field_id,
                                $field_option,
                                $field_value_ty
                            )
                        }
                    )*

                    /// Returns all fields and records a dependency on the whole input record.
                    pub fn $fields_fn<'db, $Db>(self, db: &'db $Db) -> &'db $Fields
                    where
                        $Db: ?Sized + $zalsa::Database,
                        {
                        ::salsa::Input::<$Fields>::fields(db, self)
                    }

                    $(
                        #[must_use]
                        $field_setter_vis fn $field_setter_id<'db, $Db>(
                            self,
                            db: &'db mut $Db,
                        ) -> impl ::salsa::Setter<FieldTy = $field_value_ty> + 'db
                        where
                            $Db: ?Sized + $zalsa::Database,
                        {
                            $crate::setup_input_struct!(
                                @set $field_kind,
                                $Configuration,
                                $Fields,
                                db,
                                self,
                                $field_id
                            )
                        }
                    )*

                    $zalsa::macro_if! { $is_singleton =>
                        pub fn try_get<$Db>(db: &$Db) -> Option<Self>
                        where
                            $Db: ?Sized + $zalsa::Database,
                        {
                            let zalsa = db.zalsa();
                            $Configuration::ingredient_(zalsa).get_singleton_input(zalsa)
                        }

                        #[track_caller]
                        pub fn get<$Db>(db: &$Db) -> Self
                        where
                            $Db: ?Sized + $zalsa::Database,
                        {
                            Self::try_get(db).unwrap()
                        }
                    }

                    /// Default debug formatting for this struct.
                    pub fn default_debug_fmt(
                        this: Self,
                        f: &mut ::std::fmt::Formatter<'_>,
                    ) -> ::std::fmt::Result
                    where
                        $(for<'__trivial_bounds> $field_ty: ::std::fmt::Debug),*
                    {
                        ::std::fmt::Debug::fmt($zalsa::Struct::as_repr(&this), f)
                    }
                }

                impl $zalsa_struct::HasBuilder for $Struct {
                    type Builder = builder::$Builder;
                }

                impl builder::$Builder {
                    /// Creates the new input with the set values.
                    #[must_use]
                    pub fn new<$Db>(self, db: &$Db) -> $Struct
                    where
                        $Db: ?Sized + $zalsa::Database,
                    {
                        let (values, durabilities) = builder::builder_into_inner(self);
                        let fields = $Fields {
                            $(
                                $field_id: $crate::setup_input_struct!(
                                    @field_init $field_kind,
                                    db,
                                    values.$field_index,
                                    durabilities[$field_index]
                                ),
                            )*
                        };
                        let (zalsa, zalsa_local) = db.zalsas();
                        let revision = zalsa.current_revision();
                        let durability = durabilities.iter().copied().min().unwrap_or_default();
                        $Configuration::ingredient_(zalsa).new_input(
                            zalsa,
                            zalsa_local,
                            fields,
                            [revision],
                            [durability],
                        )
                    }
                }

                mod builder {
                    use super::*;
                    use ::salsa::plumbing as $zalsa;

                    pub(super) fn new_builder($($field_id: $field_value_ty),*) -> $Builder {
                        $Builder {
                            fields: ($($field_id,)*),
                            durabilities: [$zalsa::Durability::default(); $N],
                        }
                    }

                    pub(super) fn builder_into_inner(
                        builder: $Builder,
                    ) -> (($($field_value_ty,)*), [$zalsa::Durability; $N]) {
                        (builder.fields, builder.durabilities)
                    }

                    #[must_use]
                    pub struct $Builder {
                        fields: ($($field_value_ty,)*),
                        durabilities: [$zalsa::Durability; $N],
                    }

                    impl $Builder {
                        /// Sets the durability of all fields.
                        pub fn durability(mut self, durability: $zalsa::Durability) -> Self {
                            self.durabilities = [durability; $N];
                            self
                        }

                        $($zalsa::maybe_default_tt! { $field_option =>
                            /// Sets this field's value.
                            #[must_use]
                            pub fn $field_id(mut self, value: $field_value_ty) -> Self {
                                self.fields.$field_index = value;
                                self
                            }
                        })*

                        $(
                            /// Sets this field's durability.
                            #[must_use]
                            pub fn $field_durability_id(
                                mut self,
                                durability: $zalsa::Durability,
                            ) -> Self {
                                self.durabilities[$field_index] = durability;
                                self
                            }
                        )*
                    }
                }
            }
        };
    };

    (@field_init input, $db:ident, $value:expr, $durability:expr) => {
        ::salsa::InputField::new_with_durability($db, $value, $durability)
    };
    (@field_init plain, $db:ident, $value:expr, $durability:expr) => {
        $value
    };

    (@get input, $Configuration:ident, $Fields:ident, $db:ident, $this:ident, $field_id:ident, $field_option:tt, $field_ty:ty) => {{
        let fields = ::salsa::Input::<$Fields>::fields_untracked($db, $this);
        let value = fields.$field_id.get($db);
        $crate::return_mode_expression!($field_option, $field_ty, value,)
    }};
    (@get plain, $Configuration:ident, $Fields:ident, $db:ident, $this:ident, $field_id:ident, $field_option:tt, $field_ty:ty) => {{
        let fields = ::salsa::Input::<$Fields>::fields($db, $this);
        $crate::return_mode_expression!($field_option, $field_ty, &fields.$field_id,)
    }};

    (@set input, $Configuration:ident, $Fields:ident, $db:ident, $this:ident, $field_id:ident) => {{
        let handle = {
            ::salsa::Input::<$Fields>::fields_untracked($db, $this)
                .$field_id
        };
        handle.set($db)
    }};
    (@set plain, $Configuration:ident, $Fields:ident, $db:ident, $this:ident, $field_id:ident) => {{
        let zalsa = $db.zalsa_mut();
        zalsa.new_revision();
        let index = zalsa.lookup_jar_by_type::<::salsa::plumbing::input::JarImpl<$Configuration>>();
        let (ingredient, runtime) = zalsa.lookup_ingredient_mut(index);
        let ingredient = ingredient.assert_type_mut::<::salsa::plumbing::input::IngredientImpl<$Configuration>>();
        ::salsa::plumbing::input::SetterImpl::new(runtime, $this, 0, ingredient, |fields, value| {
            ::std::mem::replace(&mut fields.$field_id, value)
        })
    }};

}
