use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{punctuated::Punctuated, Data, DeriveInput, Fields, LitStr, Token};

use crate::helpers::{non_enum_error, HasStrumVariantProperties, HasTypeProperties};

pub fn to_string_inner(ast: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &ast.ident;
    let (impl_generics, ty_generics, where_clause) = ast.generics.split_for_impl();
    let variants = match &ast.data {
        Data::Enum(v) => &v.variants,
        _ => return Err(non_enum_error()),
    };

    let type_properties = ast.get_type_properties()?;
    let mut arms = Vec::new();
    for variant in variants {
        let ident = &variant.ident;
        let variant_properties = variant.get_variant_properties()?;

        if variant_properties.disabled.is_some() {
            continue;
        }

        // display variants like Green("lime") as "lime"
        if variant_properties.to_string.is_none() && variant_properties.default.is_some() {
            match &variant.fields {
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    arms.push(quote! { #name::#ident(ref s) => ::std::string::String::from(s) });
                    continue;
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        variant,
                        "Default only works on newtype structs with a single String field",
                    ))
                }
            }
        }

        // Look at all the serialize attributes.
        let output = variant_properties.get_preferred_name(type_properties.case_style);

        let params = match &variant.fields {
            Fields::Unit => quote! {},
            Fields::Unnamed(..) => quote! { (..) },
            Fields::Named(field_names) => {
                // Transform struct params '{ name: String, age: u8 }' to '{ ref name, ref age }'
                let names: Punctuated<TokenStream, Token!(,)> = field_names
                    .named
                    .iter()
                    .map(|field| {
                        let ident = field.ident.as_ref().unwrap();
                        quote! { ref #ident }
                    })
                    .collect();

                quote! { {#names} }
            }
        };

        let arm = match variant.fields {
            Fields::Named(ref field_names) => {
                let used_vars = capture_format_string_idents(&output)?;
                // Create args like 'name = name, age = age' for format macro
                let args: Punctuated<_, Token!(,)> = field_names
                    .named
                    .iter()
                    .filter_map(|field| {
                        let ident = field.ident.as_ref().unwrap();
                        // Only contain variables that are used in format string
                        if !used_vars.contains(ident) {
                            None
                        } else {
                            Some(quote! { #ident = #ident })
                        }
                    })
                    .collect();

                quote! {
                    #[allow(unused_variables)]
                    #name::#ident #params => format!(#output, #args)
                }
            }
            _ => quote! { #name::#ident #params => ::std::string::String::from(#output) },
        };
        arms.push(arm);
    }

    if arms.len() < variants.len() {
        arms.push(quote! { _ => panic!("to_string() called on disabled variant.") });
    }

    Ok(quote! {
        #[allow(clippy::use_self)]
        impl #impl_generics ::std::string::ToString for #name #ty_generics #where_clause {
            fn to_string(&self) -> ::std::string::String {
                match *self {
                    #(#arms),*
                }
            }
        }
    })
}

fn capture_format_string_idents(string_literal: &LitStr) -> syn::Result<Vec<Ident>> {
    // Remove escaped brackets
    let format_str = string_literal.value().replace("{{", "").replace("}}", "");

    let mut new_var_start_index: Option<usize> = None;
    let mut var_used: Vec<Ident> = Vec::new();

    for (i, chr) in format_str.chars().enumerate() {
        if chr == '{' {
            if new_var_start_index.is_some() {
                return Err(syn::Error::new_spanned(
                    string_literal,
                    "Bracket opened without closing previous bracket",
                ));
            }
            new_var_start_index = Some(i);
            continue;
        }

        if chr == '}' {
            let start_index = new_var_start_index.take().ok_or(syn::Error::new_spanned(
                string_literal,
                "Bracket closed without previous opened bracket",
            ))?;

            let inside_brackets = &format_str[start_index + 1..i];
            let ident_str = inside_brackets.split(":").next().unwrap();
            let ident = syn::parse_str::<Ident>(ident_str).map_err(|_| {
                syn::Error::new_spanned(
                    string_literal,
                    "Invalid identifier inside format string bracket",
                )
            })?;
            var_used.push(ident);
        }
    }

    Ok(var_used)
}
