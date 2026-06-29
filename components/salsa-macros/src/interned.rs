use proc_macro2::TokenStream;

use crate::hygiene::Hygiene;
use crate::options::{AllowedOptions, AllowedPersistOptions, Options};
use crate::salsa_struct::{SalsaStruct, SalsaStructAllowedOptions};
use crate::{db_lifetime, token_stream_with_error};

/// For an entity struct `Foo` with fields `f1: T1, ..., fN: TN`, we generate...
///
/// * the "id struct" `struct Foo(salsa::Id)`
/// * the entity ingredient, which maps the id fields to the `Id`
/// * for each value field, a function ingredient
pub(crate) fn interned(
    args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let args = syn::parse_macro_input!(args as InternedArgs);
    let hygiene = Hygiene::from1(&input);
    let struct_item = parse_macro_input!(input as syn::ItemStruct);
    let m = Macro {
        hygiene,
        args,
        struct_item,
    };
    match m.try_macro() {
        Ok(v) => v.into(),
        Err(e) => token_stream_with_error(input, e),
    }
}

type InternedArgs = Options<InternedStruct>;

struct InternedStruct;

impl AllowedOptions for InternedStruct {
    const RETURNS: bool = false;

    const SPECIFY: bool = false;

    const NO_EQ: bool = false;

    const DEBUG: bool = true;

    const NO_LIFETIME: bool = true;

    const BARE: bool = true;

    const NON_UPDATE_TYPES: bool = true;

    const SINGLETON: bool = false;

    const FIELDS: bool = true;

    const DATA: bool = true;

    const DB: bool = false;

    const CYCLE_FN: bool = false;

    const CYCLE_INITIAL: bool = false;

    const CYCLE_RESULT: bool = false;

    const LRU: bool = false;

    const CONSTRUCTOR_NAME: bool = true;

    const ID: bool = true;

    const REVISIONS: bool = true;

    const HEAP_SIZE: bool = true;

    const SELF_TY: bool = false;

    const PERSIST: AllowedPersistOptions = AllowedPersistOptions::AllowedValue;
}

impl SalsaStructAllowedOptions for InternedStruct {
    const KIND: &'static str = "interned";

    const ALLOW_MAYBE_UPDATE: bool = false;

    const ALLOW_TRACKED: bool = false;

    const HAS_LIFETIME: bool = true;

    const ELIDABLE_LIFETIME: bool = true;

    const ALLOW_DEFAULT: bool = false;
}

struct Macro {
    hygiene: Hygiene,
    args: InternedArgs,
    struct_item: syn::ItemStruct,
}

impl Macro {
    #[allow(non_snake_case)]
    fn try_macro(&self) -> syn::Result<TokenStream> {
        let salsa_struct = SalsaStruct::new(&self.struct_item, &self.args)?;

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
        let field_indices = salsa_struct.field_indices();
        let field_vis = salsa_struct.field_vis();
        let field_getter_ids = salsa_struct.field_getter_ids();
        let field_options = salsa_struct.field_options();
        let field_tys = salsa_struct.field_tys();
        let field_indexed_tys = salsa_struct.field_indexed_tys();
        let field_unused_attrs = salsa_struct.field_attrs();
        let storage_field_attrs = salsa_struct.storage_field_attrs();
        let generate_debug_impl = salsa_struct.generate_debug_impl();
        let has_lifetime = salsa_struct.generate_lifetime();
        let id = salsa_struct.id();
        let revisions = salsa_struct.revisions();

        let (db_lt_arg, cfg, interior_lt) = if has_lifetime {
            (
                Some(db_lt.clone()),
                quote!(#struct_ident<'static>),
                db_lt.clone(),
            )
        } else {
            let span = syn::spanned::Spanned::span(&self.struct_item.generics);
            let static_lifetime = syn::Lifetime {
                apostrophe: span,
                ident: syn::Ident::new("static", span),
            };

            (None, quote!(#struct_ident), static_lifetime)
        };

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
            field_tys.iter().copied(),
            None,
        );
        let fields_debug = salsa_struct.fields_debug_impl(
            fields_generics.clone().unwrap_or_default(),
            fields_type.clone(),
            field_tys.iter().copied(),
        );
        let heap_size_fn = self.args.heap_size_fn.iter();
        let generate_methods = self.args.bare.is_none();

        let zalsa = self.hygiene.ident("zalsa");
        let zalsa_struct = self.hygiene.ident("zalsa_struct");
        let Configuration = self.hygiene.ident("Configuration");
        let CACHE = self.hygiene.ident("CACHE");
        let Db = self.hygiene.ident("Db");

        let assert_types_are_update = if self.args.non_update_types.is_none() {
            crate::update::assert_update(&db_lt, &zalsa, field_tys.iter().map(|ty| (**ty).clone()))
        } else {
            quote! {}
        };

        Ok(crate::debug::dump_tokens(
            struct_ident,
            quote! {
                #fields_attrs
                #(#fields_heap_size_attrs)*
                #[derive(Clone, PartialEq, Eq, Hash, salsa::InternedData)]
                #vis struct #fields_ident #fields_generics {
                    #(
                        #(#storage_field_attrs)*
                        #field_vis #field_ids: #field_tys,
                    )*
                }

                #fields_debug
                #fields_serialize
                #fields_deserialize

                salsa::plumbing::setup_interned_struct!(
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
                    StructWithStatic: #cfg,
                    db_lt: #db_lt,
                    db_lt_arg: #db_lt_arg,
                    id: #id,
                    revisions: #(#revisions)*,
                    interior_lt: #interior_lt,
                    new_fn: #new_fn,
                    field_options: [#(#field_options),*],
                    field_ids: [#(#field_ids),*],
                    field_getters: [#(#field_vis #field_getter_ids),*],
                    field_tys: [#(#field_tys),*],
                    field_indices: [#(#field_indices),*],
                    field_indexed_tys: [#(#field_indexed_tys),*],
                    field_attrs: [#([#(#field_unused_attrs),*]),*],
                    generate_debug_impl: #generate_debug_impl,
                    generate_methods: #generate_methods,
                    heap_size_fn: #(#heap_size_fn)*,
                    persist: #persist,
                    serialize_fn: #(#serialize_fn)*,
                    deserialize_fn: #(#deserialize_fn)*,
                    assert_types_are_update: { #assert_types_are_update },
                    unused_names: [
                        #zalsa,
                        #zalsa_struct,
                        #Configuration,
                        #CACHE,
                        #Db,
                    ]
                );
            },
        ))
    }
}
