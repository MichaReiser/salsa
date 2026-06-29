use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::visit::Visit;
use syn::{
    Data, DeriveInput, Error, Fields, GenericArgument, GenericParam, Ident, PathArguments, Result,
    Type,
};

#[derive(Clone, Copy)]
pub(crate) enum DataKind {
    Tracked,
    Interned,
    Input,
}

pub(crate) fn derive(kind: DataKind, input: DeriveInput) -> Result<TokenStream2> {
    validate_derived_data(kind, &input)?;

    let data = erased_data_type(&input.ident, &input.generics)?;
    let handle = handle_type(&kind, &data);

    let (data_impl, field_jars) = match &input.data {
        Data::Struct(item) => {
            let data_impl = match kind {
                DataKind::Tracked => tracked_data_impl(&data, &input.generics, &item.fields)?,
                DataKind::Interned | DataKind::Input => simple_data_impl(&kind, &data),
            };
            (data_impl, input_field_jars(&item.fields))
        }
        Data::Enum(item) if matches!(kind, DataKind::Input) => {
            return Err(Error::new_spanned(
                item.enum_token,
                "`InputData` can only be derived for structs",
            ));
        }
        Data::Enum(_) => (simple_data_impl(&kind, &data), Vec::new()),
        Data::Union(item) => {
            return Err(Error::new_spanned(
                item.union_token,
                "Salsa data derives do not support unions",
            ));
        }
    };

    Ok(quote! {
        const _: () = {
            use ::salsa::plumbing as __salsa;

            #data_impl

            __salsa::register_jar! {
                __salsa::ErasedJar::erase::<#handle>()
            }

            #(#field_jars)*
        };
    })
}

fn validate_derived_data(kind: DataKind, input: &DeriveInput) -> Result<()> {
    match &input.data {
        Data::Struct(data) => {
            for field in &data.fields {
                validate_derived_field(kind, field, true)?;
            }
        }
        Data::Enum(data) => {
            for variant in &data.variants {
                for field in &variant.fields {
                    validate_derived_field(kind, field, false)?;
                }
            }
        }
        Data::Union(_) => {}
    }

    Ok(())
}

fn validate_derived_field(kind: DataKind, field: &syn::Field, struct_field: bool) -> Result<()> {
    if contains_type_named(&field.ty, "TrackedField") {
        match kind {
            DataKind::Tracked if struct_field && tracked_field_type(&field.ty).is_some() => {}
            DataKind::Tracked => {
                return Err(Error::new_spanned(
                    &field.ty,
                    "`TrackedField` must be used directly as a named tracked-data field",
                ));
            }
            DataKind::Interned => {
                return Err(Error::new_spanned(
                    &field.ty,
                    "`TrackedField` cannot be stored in interned data",
                ));
            }
            DataKind::Input => {
                return Err(Error::new_spanned(
                    &field.ty,
                    "`TrackedField` cannot be stored in input data",
                ));
            }
        }
    }

    if contains_type_named(&field.ty, "InputField") {
        match kind {
            DataKind::Input if struct_field && input_field_type(&field.ty).is_some() => {}
            DataKind::Input => {
                return Err(Error::new_spanned(
                    &field.ty,
                    "`InputField` must be used directly as a named input-data field",
                ));
            }
            DataKind::Tracked => {
                return Err(Error::new_spanned(
                    &field.ty,
                    "`InputField` cannot be stored in tracked data",
                ));
            }
            DataKind::Interned => {
                return Err(Error::new_spanned(
                    &field.ty,
                    "`InputField` cannot be stored in interned data",
                ));
            }
        }
    }

    Ok(())
}

fn contains_type_named(ty: &Type, name: &str) -> bool {
    struct Finder<'a> {
        name: &'a str,
        found: bool,
    }

    impl Visit<'_> for Finder<'_> {
        fn visit_type_path(&mut self, path: &syn::TypePath) {
            self.found |= path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == self.name);
            syn::visit::visit_type_path(self, path);
        }
    }

    let mut finder = Finder { name, found: false };
    finder.visit_type(ty);
    finder.found
}

fn erased_data_type(ident: &Ident, generics: &syn::Generics) -> Result<TokenStream2> {
    let lifetimes = generics
        .params
        .iter()
        .map(|param| match param {
            GenericParam::Lifetime(_) => Ok(quote!('static)),
            GenericParam::Type(param) => Err(Error::new_spanned(
                param,
                "Salsa data derives do not support type parameters",
            )),
            GenericParam::Const(param) => Err(Error::new_spanned(
                param,
                "Salsa data derives do not support const parameters",
            )),
        })
        .collect::<Result<Vec<_>>>()?;

    if lifetimes.is_empty() {
        Ok(quote!(#ident))
    } else {
        Ok(quote!(#ident<#(#lifetimes),*>))
    }
}

fn handle_type(kind: &DataKind, data: &TokenStream2) -> TokenStream2 {
    match kind {
        DataKind::Tracked => quote!(::salsa::Tracked<'static, #data>),
        DataKind::Interned => quote!(::salsa::Interned<'static, #data>),
        DataKind::Input => quote!(::salsa::Input<#data>),
    }
}

fn simple_data_impl(kind: &DataKind, data: &TokenStream2) -> TokenStream2 {
    if matches!(kind, DataKind::Tracked) {
        return plain_tracked_data_impl(data);
    }

    let data_trait = match kind {
        DataKind::Tracked => unreachable!(),
        DataKind::Interned => quote!(__salsa::generic::InternedData),
        DataKind::Input => quote!(__salsa::generic::InputData),
    };

    quote! {
        #[allow(clippy::all)]
        impl #data_trait for #data {
            const INGREDIENT_CACHE: &'static __salsa::IngredientIndexCache = {
                static CACHE: __salsa::IngredientIndexCache =
                    __salsa::IngredientIndexCache::new();
                &CACHE
            };
        }
    }
}

fn plain_tracked_data_impl(data: &TokenStream2) -> TokenStream2 {
    quote! {
        #[allow(clippy::all)]
        impl __salsa::generic::TrackedData for #data {
            type Revisions = [__salsa::AtomicRevision; 0];

            const TRACKED_FIELD_NAMES: &'static [&'static str] = &[];
            const INGREDIENT_CACHE: &'static __salsa::IngredientIndexCache = {
                static CACHE: __salsa::IngredientIndexCache =
                    __salsa::IngredientIndexCache::new();
                &CACHE
            };

            fn bind_tracked_fields(
                _: __salsa::IngredientIndex,
                _: __salsa::Id,
                _: &mut <Self as __salsa::Update>::Rebind<'_>,
            ) {
            }

            fn identity_fields(
                fields: &<Self as __salsa::Update>::Rebind<'_>,
            ) -> impl ::std::hash::Hash {
                fields
            }

            fn new_revisions(_: __salsa::Revision) -> Self::Revisions {
                []
            }

            unsafe fn update_fields<'db>(
                _: __salsa::Revision,
                _: &Self::Revisions,
                old_fields: *mut <Self as __salsa::Update>::Rebind<'db>,
                new_fields: <Self as __salsa::Update>::Rebind<'db>,
            ) -> bool {
                // SAFETY: forwarded from the `TrackedData` contract.
                unsafe {
                    <<Self as __salsa::Update>::Rebind<'db> as __salsa::Update>::maybe_update(
                        old_fields,
                        new_fields,
                    )
                }
            }
        }
    }
}

fn tracked_data_impl(
    data: &TokenStream2,
    generics: &syn::Generics,
    item_fields: &Fields,
) -> Result<TokenStream2> {
    let Fields::Named(fields) = item_fields else {
        if item_fields
            .iter()
            .any(|field| tracked_field_type(&field.ty).is_some())
        {
            return Err(Error::new_spanned(
                item_fields,
                "tracked data containing `TrackedField` must use named fields",
            ));
        }
        return Ok(simple_data_impl(&DataKind::Tracked, data));
    };

    let tracked = fields
        .named
        .iter()
        .filter(|field| tracked_field_type(&field.ty).is_some())
        .collect::<Vec<_>>();
    let untracked = fields
        .named
        .iter()
        .filter(|field| tracked_field_type(&field.ty).is_none())
        .collect::<Vec<_>>();
    let tracked_ids = tracked.iter().map(|field| field.ident.as_ref().unwrap());
    let tracked_ids_for_bind = tracked_ids.clone();
    let tracked_ids_for_update = tracked_ids.clone();
    let tracked_updates = tracked
        .iter()
        .map(|field| tracked_field_update(field))
        .collect::<Result<Vec<_>>>()?;
    let untracked_ids = untracked.iter().map(|field| field.ident.as_ref().unwrap());
    let untracked_ids_for_update = untracked_ids.clone();
    let untracked_updates = untracked
        .iter()
        .map(|field| field_update(field, &field.ty))
        .collect::<Result<Vec<_>>>()?;
    let tracked_indices = (0..tracked.len()).map(syn::Index::from);
    let tracked_indices_for_bind = tracked_indices.clone();
    let tracked_indices_for_update = tracked_indices.clone();
    let tracked_count = tracked.len();
    let update_lifetime = generics
        .lifetimes()
        .next()
        .map(|lifetime| lifetime.lifetime.clone())
        .unwrap_or_else(|| syn::parse_quote!('__salsa_db));

    Ok(quote! {
        #[allow(clippy::all)]
        impl __salsa::generic::TrackedData for #data {
            type Revisions = [__salsa::AtomicRevision; #tracked_count];

            const TRACKED_FIELD_NAMES: &'static [&'static str] = &[
                #(stringify!(#tracked_ids),)*
            ];

            const INGREDIENT_CACHE: &'static __salsa::IngredientIndexCache = {
                static CACHE: __salsa::IngredientIndexCache =
                    __salsa::IngredientIndexCache::new();
                &CACHE
            };

            fn bind_tracked_fields(
                ingredient_index: __salsa::IngredientIndex,
                id: __salsa::Id,
                fields: &mut <Self as __salsa::Update>::Rebind<'_>,
            ) {
                #(
                    fields.#tracked_ids_for_bind.bind(
                        ingredient_index,
                        id,
                        #tracked_indices_for_bind,
                    );
                )*
            }

            fn identity_fields(
                fields: &<Self as __salsa::Update>::Rebind<'_>,
            ) -> impl ::std::hash::Hash {
                (#(&fields.#untracked_ids,)*)
            }

            fn new_revisions(current_revision: __salsa::Revision) -> Self::Revisions {
                ::std::array::from_fn(|_| __salsa::AtomicRevision::new(current_revision))
            }

            unsafe fn update_fields<#update_lifetime>(
                current_revision: __salsa::Revision,
                revisions: &Self::Revisions,
                old_fields: *mut <Self as __salsa::Update>::Rebind<#update_lifetime>,
                new_fields: <Self as __salsa::Update>::Rebind<#update_lifetime>,
            ) -> bool {
                use __salsa::UpdateFallback as _;

                #(
                    if unsafe {
                        __salsa::TrackedField::maybe_update(
                            ::std::ptr::addr_of_mut!((*old_fields).#tracked_ids_for_update),
                            new_fields.#tracked_ids_for_update,
                            #tracked_updates,
                        )
                    } {
                        revisions[#tracked_indices_for_update].store(current_revision);
                    }
                )*

                false #(
                    | unsafe {
                        (#untracked_updates)(
                            ::std::ptr::addr_of_mut!((*old_fields).#untracked_ids_for_update),
                            new_fields.#untracked_ids_for_update,
                        )
                    }
                )*
            }
        }
    })
}

fn input_field_jars(fields: &Fields) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| input_field_type(&field.ty))
        .map(|ty| {
            quote! {
                __salsa::register_jar! {
                    __salsa::ErasedJar::erase::<::salsa::InputField<#ty>>()
                }
            }
        })
        .collect()
}

fn tracked_field_update(field: &syn::Field) -> Result<TokenStream2> {
    let ty = tracked_field_type(&field.ty).unwrap();
    field_update(field, ty)
}

fn field_update(field: &syn::Field, ty: &Type) -> Result<TokenStream2> {
    let mut no_eq = false;
    let mut maybe_update = None;

    for attr in &field.attrs {
        if !attr.path().is_ident("salsa") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("no_eq") {
                no_eq = true;
                Ok(())
            } else if meta.path.is_ident("maybe_update") {
                maybe_update = Some(meta.value()?.parse::<syn::Expr>()?);
                Ok(())
            } else {
                Err(meta.error("unsupported `TrackedData` field option"))
            }
        })?;
    }

    if no_eq && maybe_update.is_some() {
        return Err(Error::new_spanned(
            field,
            "`no_eq` and `maybe_update` cannot be combined",
        ));
    }

    Ok(if no_eq {
        quote!(__salsa::always_update::<#ty>)
    } else if let Some(maybe_update) = maybe_update {
        quote!({
            let maybe_update: unsafe fn(*mut #ty, #ty) -> bool = #maybe_update;
            maybe_update
        })
    } else {
        quote!(__salsa::UpdateDispatch::<#ty>::maybe_update)
    })
}

fn input_field_type(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "InputField" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

fn tracked_field_type(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "TrackedField" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}
