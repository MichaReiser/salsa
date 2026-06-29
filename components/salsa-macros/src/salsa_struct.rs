//! Common code for `#[salsa::interned]`, `#[salsa::input]`, and
//! `#[salsa::tracked]` decorators.
//!
//! Example of usage:
//!
//! ```rust,ignore
//! #[salsa::interned(fields = TyFields)]
//! #[derive(Eq, PartialEq, Hash, Debug, Clone)]
//! struct Ty0 {
//!    field1: Type1,
//!    #[ref] field2: Type2,
//!    ...
//! }
//! ```
//! For a Salsa struct `Foo`, we generate:
//!
//! * the actual struct: `struct Foo(Id);`
//! * a struct containing the stored fields (hidden unless `fields = Name` is set)
//! * constructor function: `impl Foo { fn new(db: &crate::Db, field1: Type1, ..., fieldN: TypeN) -> Self { ... } }
//! * field accessors: `impl Foo { fn field1(&self) -> Type1 { self.field1.clone() } }`
//!     * if the field is `ref`, we generate `fn field1(&self) -> &Type1`
//! * a `fields` method that borrows the stored fields in one operation

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::ToTokens;
use syn::parse::ParseStream;
use syn::{GenericArgument, PathArguments, ext::IdentExt, spanned::Spanned};

use crate::db_lifetime;
use crate::hygiene::Hygiene;
use crate::options::{AllowedOptions, Options};

pub(crate) struct SalsaStruct<'s, A: SalsaStructAllowedOptions> {
    struct_item: &'s syn::ItemStruct,
    args: &'s Options<A>,
    fields: Vec<SalsaField<'s>>,
}

pub(crate) struct FieldsTypes {
    pub(crate) generics: Option<TokenStream>,
    pub(crate) impl_lifetime: Option<syn::Lifetime>,
    pub(crate) ty: TokenStream,
    pub(crate) static_ty: TokenStream,
    pub(crate) rebind_lifetime: syn::Lifetime,
    pub(crate) rebind_ty: TokenStream,
}

pub(crate) trait SalsaStructAllowedOptions: AllowedOptions {
    /// The kind of struct (e.g., interned, input, tracked).
    const KIND: &'static str;

    /// Are `#[maybe_update]` fields allowed?
    const ALLOW_MAYBE_UPDATE: bool;

    /// Are `#[tracked]` fields allowed?
    const ALLOW_TRACKED: bool;

    /// Does this kind of struct have a `'db` lifetime?
    const HAS_LIFETIME: bool;

    /// Can this struct elide the `'db` lifetime?
    const ELIDABLE_LIFETIME: bool;

    /// Are `#[default]` fields allowed?
    const ALLOW_DEFAULT: bool;
}

pub(crate) struct SalsaField<'s> {
    pub(crate) field: &'s syn::Field,

    pub(crate) has_tracked_attr: bool,
    pub(crate) has_default_attr: bool,
    pub(crate) returns: syn::Ident,
    pub(crate) has_no_eq_attr: bool,
    pub(crate) maybe_update_attr: Option<(syn::Path, syn::Expr)>,
    get_name: syn::Ident,
    set_name: syn::Ident,
    unknown_attrs: Vec<&'s syn::Attribute>,
}

const BANNED_FIELD_NAMES: &[&str] = &["from", "new"];
const ALLOWED_RETURN_MODES: &[&str] = &["copy", "clone", "ref", "deref", "as_ref", "as_deref"];

#[allow(clippy::type_complexity)]
pub(crate) const FIELD_OPTION_ATTRIBUTES: &[(
    &str,
    fn(&syn::Attribute, &mut SalsaField) -> syn::Result<()>,
)] = &[
    ("tracked", |_, ef| {
        ef.has_tracked_attr = true;
        Ok(())
    }),
    ("default", |_, ef| {
        ef.has_default_attr = true;
        Ok(())
    }),
    ("returns", |attr, ef| {
        ef.returns = attr.parse_args_with(syn::Ident::parse_any)?;
        Ok(())
    }),
    ("no_eq", |_, ef| {
        ef.has_no_eq_attr = true;
        Ok(())
    }),
    ("get", |attr, ef| {
        ef.get_name = attr.parse_args()?;
        Ok(())
    }),
    ("set", |attr, ef| {
        ef.set_name = attr.parse_args()?;
        Ok(())
    }),
    ("maybe_update", |attr, ef| {
        ef.maybe_update_attr = Some(attr.parse_args_with(|parser: ParseStream| {
            let expr = parser.parse::<syn::Expr>()?;
            Ok((attr.path().clone(), expr))
        })?);
        Ok(())
    }),
];

impl<'s, A> SalsaStruct<'s, A>
where
    A: SalsaStructAllowedOptions,
{
    pub fn new(struct_item: &'s syn::ItemStruct, args: &'s Options<A>) -> syn::Result<Self> {
        if args.bare.is_some() && args.fields.is_none() {
            return Err(syn::Error::new_spanned(
                &struct_item.ident,
                "`fields = Name` is required in bare mode",
            ));
        }

        let syn::Fields::Named(n) = &struct_item.fields else {
            return Err(syn::Error::new_spanned(
                &struct_item.ident,
                "must have named fields for a struct",
            ));
        };

        let fields = n
            .named
            .iter()
            .map(SalsaField::new)
            .collect::<syn::Result<_>>()?;

        let this = Self {
            struct_item,
            args,
            fields,
        };

        this.maybe_disallow_maybe_update_fields()?;
        this.maybe_disallow_tracked_fields()?;
        this.maybe_disallow_default_fields()?;

        this.check_generics()?;

        Ok(this)
    }

    /// Returns the `constructor_name` in `Options` if it is `Some`, else `new`
    pub(crate) fn constructor_name(&self) -> syn::Ident {
        match self.args.constructor_name.clone() {
            Some(name) => name,
            None => Ident::new("new", self.struct_item.ident.span()),
        }
    }

    pub(crate) fn fields_ident(&self, hygiene: &Hygiene) -> syn::Ident {
        self.args
            .fields
            .clone()
            .unwrap_or_else(|| hygiene.scoped_ident(&self.struct_item.ident, "Fields"))
    }

    pub(crate) fn fields_attrs(&self) -> TokenStream {
        if self.args.fields.is_none() {
            quote!(#[doc(hidden)])
        } else {
            TokenStream::new()
        }
    }

    pub(crate) fn fields_types(
        &self,
        hygiene: &Hygiene,
        fields_ident: &Ident,
        db_lifetime: &syn::Lifetime,
    ) -> FieldsTypes {
        let has_lifetime = self.fields_use_lifetime(db_lifetime);
        let generics = has_lifetime.then(|| quote!(<#db_lifetime>));
        let impl_lifetime = has_lifetime.then_some(db_lifetime.clone());
        let ty = if has_lifetime {
            quote!(#fields_ident<#db_lifetime>)
        } else {
            quote!(#fields_ident)
        };
        let static_ty = if has_lifetime {
            quote!(#fields_ident<'static>)
        } else {
            quote!(#fields_ident)
        };
        let rebind_ident = hygiene.ident("fields_rebind");
        let rebind_lifetime = syn::Lifetime::new(&format!("'{rebind_ident}"), rebind_ident.span());
        let rebind_ty = if has_lifetime {
            quote!(#fields_ident<#rebind_lifetime>)
        } else {
            quote!(#fields_ident)
        };

        FieldsTypes {
            generics,
            impl_lifetime,
            ty,
            static_ty,
            rebind_lifetime,
            rebind_ty,
        }
    }

    /// Returns the `id` in `Options` if it is `Some`, else `salsa::Id`.
    pub(crate) fn id(&self) -> syn::Path {
        match &self.args.id {
            Some(id) => id.clone(),
            None => parse_quote!(salsa::Id),
        }
    }

    /// Returns the `revisions` in `Options` as an optional iterator.
    pub(crate) fn revisions(&self) -> impl Iterator<Item = &syn::Expr> + '_ {
        self.args.revisions.iter()
    }

    /// Disallow `#[tracked]` attributes on the fields of this struct.
    ///
    /// If an `#[tracked]` field is found, return an error.
    ///
    /// # Parameters
    ///
    /// * `kind`, the attribute name (e.g., `input` or `interned`)
    fn maybe_disallow_maybe_update_fields(&self) -> syn::Result<()> {
        if A::ALLOW_MAYBE_UPDATE {
            return Ok(());
        }

        // Check if any field has the `#[maybe_update]` attribute.
        for ef in &self.fields {
            if ef.maybe_update_attr.is_some() {
                return Err(syn::Error::new_spanned(
                    ef.field,
                    format!(
                        "`#[maybe_update]` cannot be used with `#[salsa::{}]`",
                        A::KIND
                    ),
                ));
            }
        }

        Ok(())
    }

    /// Disallow `#[tracked]` attributes on the fields of this struct.
    ///
    /// If an `#[tracked]` field is found, return an error.
    ///
    /// # Parameters
    ///
    /// * `kind`, the attribute name (e.g., `input` or `interned`)
    fn maybe_disallow_tracked_fields(&self) -> syn::Result<()> {
        if A::ALLOW_TRACKED {
            return Ok(());
        }

        // Check if any field has the `#[tracked]` attribute.
        for ef in &self.fields {
            if ef.has_tracked_attr {
                return Err(syn::Error::new_spanned(
                    ef.field,
                    format!("`#[tracked]` cannot be used with `#[salsa::{}]`", A::KIND),
                ));
            }
        }

        Ok(())
    }

    /// Disallow `#[default]` attributes on the fields of this struct.
    ///
    /// If an `#[default]` field is found, return an error.
    ///
    /// # Parameters
    ///
    /// * `kind`, the attribute name (e.g., `input` or `interned`)
    fn maybe_disallow_default_fields(&self) -> syn::Result<()> {
        if A::ALLOW_DEFAULT {
            return Ok(());
        }

        // Check if any field has the `#[default]` attribute.
        for ef in &self.fields {
            if ef.has_default_attr {
                return Err(syn::Error::new_spanned(
                    ef.field,
                    format!("`#[default]` cannot be used with `#[salsa::{}]`", A::KIND),
                ));
            }
        }

        Ok(())
    }

    /// Check that the generic parameters look as expected for this kind of struct.
    fn check_generics(&self) -> syn::Result<()> {
        if A::HAS_LIFETIME {
            if !A::ELIDABLE_LIFETIME {
                db_lifetime::require_db_lifetime(&self.struct_item.generics)
            } else {
                Ok(())
            }
        } else {
            db_lifetime::require_no_generics(&self.struct_item.generics)
        }
    }

    pub(crate) fn field_ids(&self) -> Vec<&syn::Ident> {
        self.fields
            .iter()
            .map(|f| f.field.ident.as_ref().unwrap())
            .collect()
    }

    pub(crate) fn fields_iter(&self) -> impl Iterator<Item = &SalsaField<'s>> {
        self.fields.iter()
    }

    pub(crate) fn tracked_ids(&self) -> Vec<&syn::Ident> {
        self.tracked_fields_iter()
            .map(|(_, f)| f.field.ident.as_ref().unwrap())
            .collect()
    }

    pub(crate) fn untracked_ids(&self) -> Vec<&syn::Ident> {
        self.untracked_fields_iter()
            .map(|(_, f)| f.field.ident.as_ref().unwrap())
            .collect()
    }

    pub(crate) fn tracked_flags(&self) -> Vec<bool> {
        self.fields
            .iter()
            .map(|field| field.tracked_type().is_some())
            .collect()
    }

    pub(crate) fn input_flags(&self) -> Vec<bool> {
        self.fields
            .iter()
            .map(|field| field.input_type().is_some())
            .collect()
    }

    pub(crate) fn field_indices(&self) -> Vec<Literal> {
        (0..self.fields.len())
            .map(Literal::usize_unsuffixed)
            .collect()
    }

    pub(crate) fn num_fields(&self) -> Literal {
        Literal::usize_unsuffixed(self.fields.len())
    }

    pub(crate) fn required_fields(&self) -> Vec<TokenStream> {
        self.fields
            .iter()
            .filter_map(|f| {
                if f.has_default_attr {
                    None
                } else {
                    let ident = f.field.ident.as_ref().unwrap();
                    let ty = wrapper_inner_type(&f.field.ty, "InputField").unwrap_or(&f.field.ty);
                    Some(quote!(#ident #ty))
                }
            })
            .collect()
    }

    pub(crate) fn field_vis(&self) -> Vec<&syn::Visibility> {
        self.fields.iter().map(|f| &f.field.vis).collect()
    }

    pub(crate) fn tracked_vis(&self) -> Vec<&syn::Visibility> {
        self.tracked_fields_iter()
            .map(|(_, f)| &f.field.vis)
            .collect()
    }

    pub(crate) fn untracked_vis(&self) -> Vec<&syn::Visibility> {
        self.untracked_fields_iter()
            .map(|(_, f)| &f.field.vis)
            .collect()
    }

    pub(crate) fn field_getter_ids(&self) -> Vec<&syn::Ident> {
        self.fields.iter().map(|f| &f.get_name).collect()
    }

    pub(crate) fn tracked_getter_ids(&self) -> Vec<&syn::Ident> {
        self.tracked_fields_iter()
            .map(|(_, f)| &f.get_name)
            .collect()
    }

    pub(crate) fn untracked_getter_ids(&self) -> Vec<&syn::Ident> {
        self.untracked_fields_iter()
            .map(|(_, f)| &f.get_name)
            .collect()
    }

    pub(crate) fn field_setter_ids(&self) -> Vec<&syn::Ident> {
        self.fields.iter().map(|f| &f.set_name).collect()
    }

    pub(crate) fn fields_method_name(&self) -> syn::Ident {
        let name = if self.fields.iter().any(|field| field.get_name == "fields") {
            "salsa_fields"
        } else {
            "fields"
        };
        syn::Ident::new(name, self.struct_item.ident.span())
    }

    pub(crate) fn field_durability_ids(&self) -> Vec<syn::Ident> {
        self.fields
            .iter()
            .map(|f| quote::format_ident!("{}_durability", f.field.ident.as_ref().unwrap()))
            .collect()
    }

    pub(crate) fn field_tys(&self) -> Vec<&syn::Type> {
        self.fields.iter().map(|f| &f.field.ty).collect()
    }

    pub(crate) fn tracked_tys(&self) -> Vec<&syn::Type> {
        self.tracked_fields_iter()
            .map(|(_, field)| field.tracked_type().unwrap())
            .collect()
    }

    pub(crate) fn untracked_tys(&self) -> Vec<&syn::Type> {
        self.untracked_fields_iter()
            .map(|(_, f)| &f.field.ty)
            .collect()
    }

    pub(crate) fn field_indexed_tys(&self) -> Vec<syn::Ident> {
        self.fields
            .iter()
            .enumerate()
            .map(|(i, _)| quote::format_ident!("T{i}"))
            .collect()
    }

    pub(crate) fn field_attrs(&self) -> Vec<&[&syn::Attribute]> {
        self.fields.iter().map(|f| &*f.unknown_attrs).collect()
    }

    /// Attributes that can safely be copied to the generated storage fields.
    ///
    /// Qualified attributes may name helper attributes consumed by derives on
    /// the user's original struct, so only ordinary one-segment attributes are
    /// retained.
    pub(crate) fn storage_field_attrs(&self) -> Vec<Vec<&syn::Attribute>> {
        self.fields
            .iter()
            .map(|field| {
                field
                    .unknown_attrs
                    .iter()
                    .copied()
                    .filter(|attr| attr.path().segments.len() == 1)
                    .collect()
            })
            .collect()
    }

    pub(crate) fn tracked_field_attrs(&self) -> Vec<&[&syn::Attribute]> {
        self.tracked_fields_iter()
            .map(|f| &*f.1.unknown_attrs)
            .collect()
    }

    pub(crate) fn untracked_field_attrs(&self) -> Vec<&[&syn::Attribute]> {
        self.untracked_fields_iter()
            .map(|f| &*f.1.unknown_attrs)
            .collect()
    }

    pub(crate) fn field_options(&self) -> Vec<TokenStream> {
        self.fields.iter().map(SalsaField::options).collect()
    }

    pub(crate) fn tracked_options(&self) -> Vec<TokenStream> {
        self.tracked_fields_iter()
            .map(|(_, f)| f.options())
            .collect()
    }

    pub(crate) fn untracked_options(&self) -> Vec<TokenStream> {
        self.untracked_fields_iter()
            .map(|(_, f)| f.options())
            .collect()
    }

    pub fn generate_debug_impl(&self) -> bool {
        self.args.debug.is_some()
    }

    pub fn generate_lifetime(&self) -> bool {
        self.args.no_lifetime.is_none()
    }

    pub(crate) fn fields_use_lifetime(&self, lifetime: &syn::Lifetime) -> bool {
        struct Finder<'a> {
            lifetime: &'a syn::Lifetime,
            found: bool,
        }

        impl syn::visit::Visit<'_> for Finder<'_> {
            fn visit_lifetime(&mut self, lifetime: &syn::Lifetime) {
                self.found |= lifetime.ident == self.lifetime.ident;
            }
        }

        let mut finder = Finder {
            lifetime,
            found: false,
        };
        for field in &self.fields {
            syn::visit::Visit::visit_type(&mut finder, &field.field.ty);
        }
        finder.found
    }

    /// Generates serde impls for the retained fields struct when persistence
    /// uses Salsa's default serializer or deserializer.
    pub(crate) fn fields_serde_impls<T>(
        &self,
        serialize_generics: TokenStream,
        deserialize_generics: TokenStream,
        fields_type: TokenStream,
        field_tys: impl IntoIterator<Item = T>,
        extra_initializer: Option<TokenStream>,
    ) -> (Option<TokenStream>, Option<TokenStream>)
    where
        T: ToTokens,
    {
        let field_ids = self.field_ids();
        let field_tys = field_tys.into_iter().collect::<Vec<_>>();
        let persist = self.args.persist();

        let hidden = self.args.fields.is_none();
        let serialize = (persist
            && (hidden
                || self
                    .args
                    .persist
                    .as_ref()
                    .is_some_and(|options| options.serialize_fn.is_none())))
        .then(|| {
            quote! {
                impl #serialize_generics ::salsa::plumbing::serde::Serialize for #fields_type {
                    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                    where
                        S: ::salsa::plumbing::serde::Serializer,
                    {
                        ::salsa::plumbing::serde::Serialize::serialize(
                            &(#(&self.#field_ids,)*),
                            serializer,
                        )
                    }
                }
            }
        });

        let deserialize = (persist
            && (hidden
                || self
                    .args
                    .persist
                    .as_ref()
                    .is_some_and(|options| options.deserialize_fn.is_none())))
        .then(|| {
            quote! {
                impl #deserialize_generics ::salsa::plumbing::serde::Deserialize<'de>
                    for #fields_type
                {
                    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
                    where
                        D: ::salsa::plumbing::serde::Deserializer<'de>,
                    {
                        let (#(#field_ids,)*) =
                            <(#(#field_tys,)*) as ::salsa::plumbing::serde::Deserialize>::deserialize(deserializer)?;
                        Ok(Self {
                            #(#field_ids,)*
                            #extra_initializer
                        })
                    }
                }
            }
        });

        (serialize, deserialize)
    }

    /// Implements `Debug` for the retained fields without requiring every
    /// Salsa struct to have debug-printable fields.
    pub(crate) fn fields_debug_impl<T>(
        &self,
        impl_generics: TokenStream,
        fields_type: TokenStream,
        field_tys: impl IntoIterator<Item = T>,
    ) -> TokenStream
    where
        T: ToTokens,
    {
        let fields_name = &self.struct_item.ident;
        let field_ids = self.field_ids();
        let field_tys = field_tys.into_iter().collect::<Vec<_>>();
        let bounds = (!field_tys.is_empty()).then(|| {
            quote! {
                where
                    #(for<'__salsa_debug> #field_tys: ::core::fmt::Debug,)*
            }
        });

        quote! {
            impl #impl_generics ::core::fmt::Debug for #fields_type
            #bounds
            {
                fn fmt(
                    &self,
                    formatter: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    let mut formatter = formatter.debug_struct(stringify!(#fields_name));
                    #(let formatter = formatter.field(stringify!(#field_ids), &self.#field_ids);)*
                    formatter.finish()
                }
            }
        }
    }

    pub fn tracked_fields_iter(&self) -> impl Iterator<Item = (usize, &SalsaField<'s>)> {
        self.fields
            .iter()
            .enumerate()
            .filter(|(_, field)| field.tracked_type().is_some())
    }

    pub fn untracked_fields_iter(&self) -> impl Iterator<Item = (usize, &SalsaField<'s>)> {
        self.fields
            .iter()
            .enumerate()
            .filter(|(_, field)| field.tracked_type().is_none())
    }

    /// Returns the path to the `serialize` function as an optional iterator.
    ///
    /// This will be `None` if `persistable` returns `false`.
    pub(crate) fn serialize_fn(&self) -> impl Iterator<Item = syn::Path> + '_ {
        self.args
            .persist
            .clone()
            .map(|persist| {
                persist
                    .serialize_fn
                    .unwrap_or(parse_quote! { serde::Serialize::serialize })
            })
            .into_iter()
    }

    /// Returns the path to the `deserialize` function as an optional iterator.
    ///
    /// This will be `None` if `persistable` returns `false`.
    pub(crate) fn deserialize_fn(&self) -> impl Iterator<Item = syn::Path> + '_ {
        self.args
            .persist
            .clone()
            .map(|persist| {
                persist
                    .deserialize_fn
                    .unwrap_or(parse_quote! { serde::Deserialize::deserialize })
            })
            .into_iter()
    }
}

impl<'s> SalsaField<'s> {
    pub(crate) fn tracked_type(&self) -> Option<&syn::Type> {
        wrapper_inner_type(&self.field.ty, "TrackedField")
    }

    pub(crate) fn input_type(&self) -> Option<&syn::Type> {
        wrapper_inner_type(&self.field.ty, "InputField")
    }

    fn new(field: &'s syn::Field) -> syn::Result<Self> {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        if BANNED_FIELD_NAMES.iter().any(|n| *n == field_name_str) {
            return Err(syn::Error::new(
                field_name.span(),
                format!("the field name `{field_name_str}` is disallowed in salsa structs",),
            ));
        }

        let get_name = Ident::new(&field_name_str, field_name.span());
        let set_name = Ident::new(&format!("set_{field_name_str}",), field_name.span());
        let returns = Ident::new("clone", field.span());
        let mut result = SalsaField {
            field,
            has_tracked_attr: false,
            returns,
            has_default_attr: false,
            has_no_eq_attr: false,
            maybe_update_attr: None,
            get_name,
            set_name,
            unknown_attrs: Default::default(),
        };

        // Scan the attributes and look for the salsa attributes:
        for attr in &field.attrs {
            let mut handled = false;
            for (fa, func) in FIELD_OPTION_ATTRIBUTES {
                if attr.path().is_ident(fa) {
                    func(attr, &mut result)?;
                    handled = true;
                    break;
                }
            }
            if !handled {
                result.unknown_attrs.push(attr);
            }
        }

        // Validate return mode
        if !ALLOWED_RETURN_MODES
            .iter()
            .any(|mode| mode == &result.returns.to_string())
        {
            return Err(syn::Error::new(
                result.returns.span(),
                format!("Invalid return mode. Allowed modes are: {ALLOWED_RETURN_MODES:?}"),
            ));
        }

        Ok(result)
    }

    fn options(&self) -> TokenStream {
        let returns = &self.returns;

        let default_ident = if self.has_default_attr {
            syn::Ident::new("default", Span::call_site())
        } else {
            syn::Ident::new("required", Span::call_site())
        };

        quote!((#returns, #default_ident))
    }
}

pub(crate) fn wrapper_inner_type<'a>(ty: &'a syn::Type, wrapper: &str) -> Option<&'a syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != wrapper {
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

/// Heap-size derives are meaningful on the retained fields value as well as
/// on the nominal ID wrapper.
pub(crate) fn fields_heap_size_attrs(attrs: &[syn::Attribute]) -> Vec<&syn::Attribute> {
    use quote::ToTokens as _;

    attrs
        .iter()
        .filter(|attr| {
            (attr.path().is_ident("cfg_attr") || attr.path().is_ident("derive"))
                && attr
                    .meta
                    .to_token_stream()
                    .to_string()
                    .contains("get_size2")
        })
        .collect()
}
