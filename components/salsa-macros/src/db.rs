use proc_macro2::TokenStream;
use syn::parse::Nothing;

use crate::hygiene::Hygiene;

// Source:
//
// #[salsa::db]
// pub struct Database {
//    storage: salsa::Storage<Self>,
// }

pub(crate) fn db(
    args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let _nothing = syn::parse_macro_input!(args as Nothing);
    let hygiene = Hygiene::from(&input);
    let input = syn::parse_macro_input!(input as syn::Item);
    let db_macro = DbMacro { hygiene };
    match db_macro.try_db(input) {
        Ok(v) => v.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

struct DbMacro {
    hygiene: Hygiene,
}

#[allow(non_snake_case)]
impl DbMacro {
    fn try_db(self, input: syn::Item) -> syn::Result<TokenStream> {
        match input {
            syn::Item::Struct(input) => {
                let has_storage_impl = self.has_storage_impl(&input)?;
                Ok(quote! {
                    #has_storage_impl
                    #input
                })
            }
            syn::Item::Trait(mut input) => {
                self.add_salsa_view_method(&mut input)?;
                Ok(quote! {
                    #input
                })
            }
            syn::Item::Impl(mut input) => {
                self.add_salsa_view_method_impl(&mut input)?;
                Ok(quote! {
                    #input
                })
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    input,
                    "`db` must be applied to a struct, trait, or impl",
                ));
            }
        }
    }

    fn find_storage_field(&self, input: &syn::ItemStruct) -> syn::Result<syn::Ident> {
        let storage = "storage";
        for field in input.fields.iter() {
            if let Some(i) = &field.ident {
                if i == storage {
                    return Ok(i.clone());
                }
            } else {
                return Err(syn::Error::new_spanned(
                    field,
                    "database struct must be a braced struct (`{}`) with a field named `storage`",
                ));
            }
        }

        return Err(syn::Error::new_spanned(
            &input.ident,
            "database struct must be a braced struct (`{}`) with a field named `storage`",
        ));
    }

    fn has_storage_impl(&self, input: &syn::ItemStruct) -> syn::Result<TokenStream> {
        let storage = self.find_storage_field(input)?;
        let db = &input.ident;

        let SalsaHasStorage = self.hygiene.ident("SalsaHasStorage");
        let SalsaStorage = self.hygiene.ident("SalsaStorage");

        Ok(quote! {
            const _: () = {
                use salsa::storage::HasStorage as #SalsaHasStorage;
                use salsa::storage::Storage as #SalsaStorage;

                unsafe impl #SalsaHasStorage for #db {
                    fn storage(&self) -> &#SalsaStorage<Self> {
                        &self.#storage
                    }

                    fn storage_mut(&mut self) -> &mut #SalsaStorage<Self> {
                        &mut self.#storage
                    }
                }
            };
        })
    }

    fn add_salsa_view_method(&self, input: &mut syn::ItemTrait) -> syn::Result<()> {
        input.items.push(parse_quote! {
            fn __salsa_add_view__(&self);
        });
        Ok(())
    }

    fn add_salsa_view_method_impl(&self, input: &mut syn::ItemImpl) -> syn::Result<()> {
        let Some((_, TraitPath, _)) = &input.trait_ else {
            return Err(syn::Error::new_spanned(
                &input.self_ty,
                "impl must be on a trait",
            ));
        };
        input.items.push(parse_quote! {
            fn __salsa_add_view__(&self) {
                salsa::storage::views(self).add::<dyn #TraitPath>(|t| t, |t| t);
            }
        });
        Ok(())
    }
}
