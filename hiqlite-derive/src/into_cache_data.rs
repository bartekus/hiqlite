use quote::quote;
use syn::{Data, DeriveInput};

pub fn impl_cache_variants(input: DeriveInput) -> proc_macro::TokenStream {
    let name = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut index_matches = Vec::new();
    let mut variants_return = Vec::new();

    match input.data {
        Data::Enum(e) => {
            for (idx, var) in e.variants.iter().enumerate() {
                let id = &var.ident;
                let name = id.to_string();

                // F-077: a data-carrying variant emitted `Self::Variant => idx,`, a unit
                // pattern, and the caller got E0533 pointing at the derive. Say what the rule
                // is, at the variant that breaks it.
                if !matches!(var.fields, syn::Fields::Unit) {
                    let msg = format!(
                        "#[derive(CacheVariants)] needs unit variants: `{name}` carries data.                          A cache variant is an index into this node's caches and has no payload."
                    );
                    return quote! { ::core::compile_error!(#msg); }.into();
                }

                index_matches.push(quote! {Self::#id => #idx,});
                variants_return.push(quote! {(#idx, #name)});
            }
        }
        Data::Struct(_) | Data::Union(_) => {
            return quote! {
                ::core::compile_error!(
                    "#[derive(CacheVariants)] can only be applied to an enum"
                );
            }
            .into();
        }
    };

    quote! {
        // F-077: the generics were emitted after `for`, which is not where they go, so any
        // generic cache enum failed to compile.
        impl #impl_generics ::hiqlite::CacheVariants for #name #ty_generics #where_clause {
            #[inline(always)]
            fn hiqlite_cache_index(&self) -> usize {
                match self {
                    #(#index_matches)*
                }
            }

            fn hiqlite_cache_variants() -> &'static [(usize, &'static str)] {
                &[#(#variants_return),*]
            }
        }
    }
    .into()
}
