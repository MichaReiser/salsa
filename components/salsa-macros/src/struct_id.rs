use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, GenericArgument, PathArguments, Result, Type};

#[derive(Clone, Copy)]
enum ReprKind {
    Tracked,
    Interned,
    Input,
}

#[derive(Default)]
struct Options {
    configuration: bool,
    debug: bool,
}

pub(crate) fn derive(input: DeriveInput) -> Result<TokenStream> {
    let ident = &input.ident;
    let (field_type, construct, access) = representation(&input)?;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let options = options(&input)?;
    let data_config = data_config_impl(&input, field_type, options.configuration)?;
    let debug_impl = options.debug.then(|| {
        quote! {
            #[automatically_derived]
            impl #impl_generics ::core::fmt::Debug for #ident #type_generics #where_clause {
                fn fmt(
                    &self,
                    formatter: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    ::core::fmt::Debug::fmt(#access, formatter)
                }
            }
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::salsa::Struct for #ident #type_generics #where_clause {
            type Repr = #field_type;

            fn from_repr(_: ::salsa::plumbing::StructToken, repr: Self::Repr) -> Self {
                #construct
            }

            fn as_repr(&self) -> &Self::Repr {
                #access
            }
        }

        #data_config
        #debug_impl
    })
}

fn data_config_impl(
    input: &DeriveInput,
    repr: &Type,
    uses_wrapper_configuration: bool,
) -> Result<TokenStream> {
    let Type::Path(path) = repr else {
        return Ok(TokenStream::new());
    };
    let Some(segment) = path.path.segments.last() else {
        return Ok(TokenStream::new());
    };
    let kind = match segment.ident.to_string().as_str() {
        "Tracked" => ReprKind::Tracked,
        "Interned" => ReprKind::Interned,
        "Input" => ReprKind::Input,
        _ => return Ok(TokenStream::new()),
    };
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(Error::new_spanned(
            segment,
            "generic Salsa handle requires data",
        ));
    };
    let data = arguments
        .args
        .iter()
        .filter_map(|argument| match argument {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .next_back()
        .ok_or_else(|| Error::new_spanned(arguments, "generic Salsa handle requires data"))?;
    let erased_data = erase_lifetimes(data.clone());
    let erased_struct = erased_struct_type(input)?;

    let data_trait = match kind {
        ReprKind::Tracked => quote!(::salsa::plumbing::generic::TrackedDataConfig),
        ReprKind::Interned => quote!(::salsa::plumbing::generic::InternedDataConfig),
        ReprKind::Input => quote!(::salsa::plumbing::generic::InputDataConfig),
    };
    let config = if uses_wrapper_configuration {
        erased_struct
    } else {
        match kind {
            ReprKind::Tracked => {
                quote!(::salsa::plumbing::generic::TrackedConfig<#erased_data, #erased_struct>)
            }
            ReprKind::Interned => {
                quote!(::salsa::plumbing::generic::InternedConfig<#erased_data, #erased_struct>)
            }
            ReprKind::Input => {
                quote!(::salsa::plumbing::generic::InputConfig<#erased_data, #erased_struct>)
            }
        }
    };

    Ok(quote! {
        #[automatically_derived]
        impl #data_trait for #erased_data {
            type Configuration = #config;
        }
    })
}

fn options(input: &DeriveInput) -> Result<Options> {
    let mut options = Options::default();

    for attribute in &input.attrs {
        if !attribute.path().is_ident("salsa") {
            continue;
        }

        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("configuration") {
                if options.configuration {
                    return Err(meta.error("duplicate `configuration` option"));
                }
                options.configuration = true;
            } else if meta.path.is_ident("debug") {
                options.debug = meta.value()?.parse::<syn::LitBool>()?.value;
            }
            Ok(())
        })?;
    }

    Ok(options)
}

fn erased_struct_type(input: &DeriveInput) -> Result<TokenStream> {
    let ident = &input.ident;
    let arguments = input
        .generics
        .params
        .iter()
        .map(|parameter| match parameter {
            syn::GenericParam::Lifetime(_) => Ok(quote!('static)),
            parameter => Err(Error::new_spanned(
                parameter,
                "Salsa struct wrappers do not support type or const parameters",
            )),
        })
        .collect::<Result<Vec<_>>>()?;

    if arguments.is_empty() {
        Ok(quote!(#ident))
    } else {
        Ok(quote!(#ident<#(#arguments),*>))
    }
}

fn erase_lifetimes(mut ty: Type) -> Type {
    struct Erase;

    impl syn::visit_mut::VisitMut for Erase {
        fn visit_lifetime_mut(&mut self, lifetime: &mut syn::Lifetime) {
            *lifetime = syn::parse_quote!('static);
        }
    }

    syn::visit_mut::VisitMut::visit_type_mut(&mut Erase, &mut ty);
    ty
}

fn representation(input: &DeriveInput) -> Result<(&syn::Type, TokenStream, TokenStream)> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            input,
            "`Struct` can only be derived for a struct with one field",
        ));
    };

    match &data.fields {
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let field = &fields.unnamed[0];
            Ok((&field.ty, quote!(Self(repr)), quote!(&self.0)))
        }
        Fields::Named(fields) if fields.named.len() == 1 => {
            let field = &fields.named[0];
            let name = field.ident.as_ref().unwrap();
            Ok((&field.ty, quote!(Self { #name: repr }), quote!(&self.#name)))
        }
        fields => Err(Error::new_spanned(
            fields,
            "`Struct` requires exactly one field containing a generic Salsa handle",
        )),
    }
}
