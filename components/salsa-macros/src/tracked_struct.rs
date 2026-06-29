use proc_macro2::TokenStream;
use syn::spanned::Spanned;

use crate::db_lifetime;
use crate::hygiene::Hygiene;
use crate::options::{AllowedOptions, AllowedPersistOptions, Options};
use crate::salsa_struct::{SalsaField, SalsaStruct, SalsaStructAllowedOptions};

/// For an entity struct `Foo` with fields `f1: T1, ..., fN: TN`, we generate...
///
/// * the "id struct" `struct Foo(salsa::Id)`
/// * the entity ingredient, which maps the id fields to the `Id`
/// * for each value field, a function ingredient
pub(crate) fn tracked_struct(
    args: proc_macro::TokenStream,
    struct_item: syn::ItemStruct,
) -> syn::Result<TokenStream> {
    let hygiene = Hygiene::from2(&struct_item);
    let m = Macro {
        hygiene,
        args: syn::parse(args)?,
        struct_item,
    };
    m.try_macro()
}

type TrackedArgs = Options<TrackedStruct>;

struct TrackedStruct;

impl AllowedOptions for TrackedStruct {
    const RETURNS: bool = false;

    const SPECIFY: bool = false;

    const NO_EQ: bool = false;

    const DEBUG: bool = true;

    const NO_LIFETIME: bool = false;

    const BARE: bool = true;

    const NON_UPDATE_TYPES: bool = false;

    const SINGLETON: bool = false;

    const FIELDS: bool = true;

    const DATA: bool = false;

    const DB: bool = false;

    const CYCLE_FN: bool = false;

    const CYCLE_INITIAL: bool = false;

    const CYCLE_RESULT: bool = false;

    const LRU: bool = false;

    const CONSTRUCTOR_NAME: bool = true;

    const ID: bool = false;

    const REVISIONS: bool = false;

    const HEAP_SIZE: bool = true;

    const SELF_TY: bool = false;

    const PERSIST: AllowedPersistOptions = AllowedPersistOptions::AllowedValue;
}

impl SalsaStructAllowedOptions for TrackedStruct {
    const KIND: &'static str = "tracked";

    const ALLOW_MAYBE_UPDATE: bool = true;

    const ALLOW_TRACKED: bool = false;

    const HAS_LIFETIME: bool = true;

    const ELIDABLE_LIFETIME: bool = false;

    const ALLOW_DEFAULT: bool = false;
}

struct Macro {
    hygiene: Hygiene,
    args: TrackedArgs,
    struct_item: syn::ItemStruct,
}

impl Macro {
    #[allow(non_snake_case)]
    fn try_macro(&self) -> syn::Result<TokenStream> {
        let salsa_struct = SalsaStruct::new(&self.struct_item, &self.args)?;
        let zalsa = self.hygiene.ident("zalsa");

        let attrs = &self.struct_item.attrs;
        let fields_heap_size_attrs = crate::salsa_struct::fields_heap_size_attrs(attrs);
        let vis = &self.struct_item.vis;
        let struct_ident = &self.struct_item.ident;
        let fields_ident = salsa_struct.fields_ident(&self.hygiene);
        let fields_attrs = salsa_struct.fields_attrs();
        let db_lt = db_lifetime::db_lifetime(&self.struct_item.generics);
        let crate::salsa_struct::FieldsTypes {
            generics: fields_generics,
            impl_lifetime: fields_impl_lifetime,
            ty: fields_type,
            static_ty: fields_static_type,
            rebind_lifetime: fields_rebind_lifetime,
            rebind_ty: fields_rebind_type,
        } = salsa_struct.fields_types(&self.hygiene, &fields_ident, &db_lt);
        let fields_have_lifetime = fields_generics.is_some();
        let new_fn = salsa_struct.constructor_name();
        let fields_fn = salsa_struct.fields_method_name();

        let field_ids = salsa_struct.field_ids();
        let tracked_ids = salsa_struct.tracked_ids();
        let untracked_ids = salsa_struct.untracked_ids();

        let tracked_vis = salsa_struct.tracked_vis();
        let untracked_vis = salsa_struct.untracked_vis();

        let tracked_getter_ids = salsa_struct.tracked_getter_ids();
        let untracked_getter_ids = salsa_struct.untracked_getter_ids();

        let tracked_options = salsa_struct.tracked_options();
        let untracked_options = salsa_struct.untracked_options();

        let field_tys = salsa_struct.field_tys();
        let field_storage_tys = field_tys.iter().map(|ty| quote!(#ty)).collect::<Vec<_>>();
        let field_value_tys = field_tys
            .iter()
            .zip(salsa_struct.tracked_flags())
            .map(|(ty, tracked)| {
                if tracked {
                    let inner =
                        crate::salsa_struct::wrapper_inner_type(ty, "TrackedField").unwrap();
                    quote!(#inner)
                } else {
                    quote!(#ty)
                }
            })
            .collect::<Vec<_>>();
        let field_kinds = salsa_struct
            .tracked_flags()
            .into_iter()
            .map(|tracked| {
                syn::Ident::new(
                    if tracked { "tracked" } else { "untracked" },
                    proc_macro2::Span::call_site(),
                )
            })
            .collect::<Vec<_>>();
        let tracked_tys = salsa_struct.tracked_tys();
        let untracked_tys = salsa_struct.untracked_tys();

        let tracked_field_unused_attrs = salsa_struct.tracked_field_attrs();
        let untracked_field_unused_attrs = salsa_struct.untracked_field_attrs();
        let storage_field_attrs = salsa_struct.storage_field_attrs();
        let tracked_data_attrs = salsa_struct
            .fields_iter()
            .map(|field| {
                if field.has_no_eq_attr {
                    quote!(#[salsa(no_eq)])
                } else if let Some((_, maybe_update)) = &field.maybe_update_attr {
                    quote!(#[salsa(maybe_update = #maybe_update)])
                } else {
                    quote!()
                }
            })
            .collect::<Vec<_>>();
        let field_vis = salsa_struct.field_vis();

        let field_to_maybe_update = |(_, field): (usize, &SalsaField<'_>)| {
            let field_ty = field.tracked_type().unwrap_or(&field.field.ty);
            if field.has_no_eq_attr {
                quote! {(#zalsa::always_update::<#field_ty>)}
            } else if let Some((with_token, maybe_update)) = &field.maybe_update_attr {
                quote_spanned! { with_token.span() =>  ({ let maybe_update: unsafe fn(*mut #field_ty, #field_ty) -> bool = #maybe_update; maybe_update }) }
            } else {
                quote! {(#zalsa::UpdateDispatch::<#field_ty>::maybe_update)}
            }
        };

        let tracked_maybe_update = salsa_struct
            .tracked_fields_iter()
            .map(field_to_maybe_update);
        let untracked_maybe_update = salsa_struct
            .untracked_fields_iter()
            .map(field_to_maybe_update);

        let persist = self.args.persist();
        let serialize_fn = salsa_struct.serialize_fn();
        let deserialize_fn = salsa_struct.deserialize_fn();
        let serialize_generics = fields_have_lifetime.then(|| quote!(<#db_lt>));
        let deserialize_generics = if fields_have_lifetime {
            quote!(<'de, #db_lt>)
        } else {
            quote!(<'de>)
        };
        let (fields_serialize, fields_deserialize) = salsa_struct.fields_serde_impls(
            serialize_generics.unwrap_or_default(),
            deserialize_generics,
            fields_type.clone(),
            field_storage_tys.iter(),
            None,
        );
        let fields_debug = salsa_struct.fields_debug_impl(
            fields_generics.clone().unwrap_or_default(),
            fields_type.clone(),
            field_tys.iter().copied(),
        );
        let heap_size_fn = self.args.heap_size_fn.iter();

        let generate_debug_impl = salsa_struct.generate_debug_impl();
        let generate_methods = self.args.bare.is_none();

        let zalsa_struct = self.hygiene.ident("zalsa_struct");
        let Configuration = self.hygiene.ident("Configuration");
        let CACHE = self.hygiene.ident("CACHE");
        let Db = self.hygiene.ident("Db");
        let Revision = self.hygiene.ident("Revision");

        Ok(crate::debug::dump_tokens(
            struct_ident,
            quote! {
                #fields_attrs
                #(#fields_heap_size_attrs)*
                #[derive(salsa::TrackedData)]
                #vis struct #fields_ident #fields_generics {
                    #(
                        #tracked_data_attrs
                        #(#storage_field_attrs)*
                        #field_vis #field_ids: #field_storage_tys,
                    )*
                }

                #fields_debug
                #fields_serialize
                #fields_deserialize

                salsa::plumbing::setup_tracked_struct!(
                    attrs: [#(#attrs),*],
                    vis: #vis,
                    Struct: #struct_ident,
                    Fields: #fields_ident,
                    FieldsType: #fields_type,
                    FieldsStaticType: #fields_static_type,
                    FieldsImplGenerics: [#fields_impl_lifetime],
                    FieldsRebindLifetime: #fields_rebind_lifetime,
                    FieldsRebindType: #fields_rebind_type,
                    fields_fn: #fields_fn,
                    db_lt: #db_lt,
                    new_fn: #new_fn,

                    field_ids: [#(#field_ids),*],
                    tracked_ids: [#(#tracked_ids),*],
                    untracked_ids: [#(#untracked_ids),*],
                    field_kinds: [#(#field_kinds),*],

                    tracked_getters: [#(#tracked_vis #tracked_getter_ids),*],
                    untracked_getters: [#(#untracked_vis #untracked_getter_ids),*],

                    field_tys: [#(#field_value_tys),*],
                    tracked_tys: [#(#tracked_tys),*],
                    untracked_tys: [#(#untracked_tys),*],


                    tracked_maybe_updates: [#(#tracked_maybe_update),*],
                    untracked_maybe_updates: [#(#untracked_maybe_update),*],

                    tracked_options: [#(#tracked_options),*],
                    untracked_options: [#(#untracked_options),*],

                    tracked_field_attrs: [#([#(#tracked_field_unused_attrs),*]),*],
                    untracked_field_attrs: [#([#(#untracked_field_unused_attrs),*]),*],

                    generate_debug_impl: #generate_debug_impl,
                    generate_methods: #generate_methods,

                    heap_size_fn: #(#heap_size_fn)*,

                    persist: #persist,
                    serialize_fn: #(#serialize_fn)*,
                    deserialize_fn: #(#deserialize_fn)*,

                    unused_names: [
                        #zalsa,
                        #zalsa_struct,
                        #Configuration,
                        #CACHE,
                        #Db,
                        #Revision,
                    ]
                );
            },
        ))
    }
}
