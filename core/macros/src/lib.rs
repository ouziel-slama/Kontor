extern crate proc_macro;

use std::fs;

use anyhow::{anyhow, bail};
use darling::{FromMeta, ast::NestedMeta};
use heck::{ToSnakeCase, ToUpperCamelCase};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    DataEnum, DataStruct, DeriveInput, Error, Fields, Ident, PathArguments, Result, Type, TypePath,
    parse_macro_input, spanned::Spanned,
};
use wit_parser::{Resolve, TypeDefKind, WorldItem, WorldKey};

#[proc_macro_derive(Store)]
pub fn derive_store(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;

    let body = match &input.data {
        syn::Data::Struct(data_struct) => generate_struct_body(data_struct, name),
        syn::Data::Enum(data_enum) => generate_enum_body(data_enum, name),
        syn::Data::Union(_) => Err(Error::new(
            name.span(),
            "Store derive is not supported for unions",
        )),
    };

    let body = match body {
        Ok(body) => body,
        Err(err) => return err.to_compile_error().into(),
    };

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let expanded = quote! {
        impl #impl_generics stdlib::Store for #name #ty_generics #where_clause {
            fn __set(ctx: &impl stdlib::WriteContext, base_path: stdlib::DotPathBuf, value: #name #ty_generics) {
                #body
            }
        }
    };

    TokenStream::from(expanded)
}

fn generate_struct_body(
    data_struct: &DataStruct,
    type_name: &Ident,
) -> Result<proc_macro2::TokenStream> {
    match &data_struct.fields {
        Fields::Named(fields) => {
            let field_sets = fields.named.iter().map(|field| {
                let field_name = field.ident.as_ref().unwrap();
                let field_name_str = field_name.to_string();
                let field_ty = &field.ty;

                if is_option_type(field_ty) {
                    quote! {
                        match value.#field_name {
                            Some(inner) => ctx.__set(base_path.push(#field_name_str), inner),
                            None => ctx.__set(base_path.push(#field_name_str), ()),
                        }
                    }
                } else {
                    quote! {
                        ctx.__set(base_path.push(#field_name_str), value.#field_name);
                    }
                }
            });
            Ok(quote! { #(#field_sets)* })
        }
        _ => Err(Error::new(
            type_name.span(),
            "Store derive only supports structs with named fields",
        )),
    }
}

fn generate_enum_body(data_enum: &DataEnum, type_name: &Ident) -> Result<proc_macro2::TokenStream> {
    let arms = data_enum.variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        let variant_name = variant_ident.to_string().to_lowercase();

        match &variant.fields {
            Fields::Unit => {
                Ok(quote! {
                    #type_name::#variant_ident => ctx.__set(base_path.push(#variant_name), ()),
                })
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                Ok(quote! {
                    #type_name::#variant_ident(inner) => ctx.__set(base_path.push(#variant_name), inner),
                })
            }
            _ => Err(Error::new(
                variant_ident.span(),
                "Store derive only supports unit or single-field tuple variants",
            )),
        }
    });

    // Collect results, propagating any errors
    let arms: Result<Vec<_>> = arms.collect();
    let arms = arms?;

    Ok(quote! {
        match value {
            #(#arms)*
        }
    })
}

fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .map(|segment| {
                segment.ident == "Option"
                    && matches!(segment.arguments, PathArguments::AngleBracketed(_))
            })
            .unwrap_or(false)
    } else {
        false
    }
}

fn to_pascal_case(name: &str) -> String {
    name.split('-')
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

#[proc_macro_derive(Wrapper)]
pub fn derive_wrapper(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;

    let body = match &input.data {
        syn::Data::Struct(data_struct) => generate_struct_wrapper(data_struct, name),
        syn::Data::Enum(data_enum) => generate_enum_wrapper(data_enum, name),
        syn::Data::Union(_) => Err(Error::new(
            name.span(),
            "Wrapper derive is not supported for unions",
        )),
    };

    let body = match body {
        Ok(body) => body,
        Err(err) => return err.to_compile_error().into(),
    };

    let (_impl_generics, _ty_generics, _where_clause) = generics.split_for_impl();
    let expanded = quote! {
        #body
    };

    TokenStream::from(expanded)
}

fn generate_struct_wrapper(
    data_struct: &DataStruct,
    type_name: &Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    match &data_struct.fields {
        Fields::Named(fields) => {
            let wrapper_name = Ident::new(&format!("{}Wrapper", type_name), type_name.span());

            let mut special_wrappers = vec![];

            let getters = fields.named.iter().map(|field| {
                let field_name = field.ident.as_ref().unwrap();
                let field_name_str = field_name.to_string();
                let field_ty = &field.ty;

                if is_map_type(field_ty) {
                    let (k_ty, v_ty) = get_map_types(field_ty)?;
                    let field_wrapper_name = Ident::new(&format!("{}{}Wrapper", type_name, to_pascal_case(&field_name.to_string())), field.span());

                    let (get_return, get_body) = if is_primitive_type(&v_ty) {
                        (quote! { Option<#v_ty> }, quote! { ctx.__get(self.base_path.push(key.to_string())) })
                    } else {
                        let v_wrapper_ty = get_wrapper_ident(&v_ty, field.span())?;
                        (quote! { Option<#v_wrapper_ty> }, quote! { ctx.__exists(&base_path.push(key.to_string())).then(|| #v_wrapper_ty::new(ctx, base_path.push(key.to_string()))) })
                    };

                    special_wrappers.push(quote! {
                        #[derive(Clone)]
                        pub struct #field_wrapper_name {
                            pub base_path: stdlib::DotPathBuf,
                        }

                        impl #field_wrapper_name {
                            pub fn get(&self, ctx: &impl stdlib::ReadContext, key: #k_ty) -> #get_return {
                                let base_path = self.base_path.push(key.to_string());
                                #get_body
                            }

                            pub fn set(&self, ctx: &impl stdlib::WriteContext, key: #k_ty, value: #v_ty) {
                                ctx.__set(self.base_path.push(key.to_string()), value)
                            }

                            pub fn load(&self, ctx: &impl stdlib::ReadContext) -> Map<#k_ty, #v_ty> {
                                Map::new(&[])
                            }
                        }
                    });

                    Ok(quote! {
                        pub fn #field_name(&self) -> #field_wrapper_name {
                            #field_wrapper_name { base_path: self.base_path.push(#field_name_str) }
                        }
                    })
                } else if is_option_type(field_ty) {
                    let inner_ty = get_option_inner_type(field_ty)?;
                    let base_path = quote! { self.base_path.push(#field_name_str) };
                    if is_primitive_type(&inner_ty) {
                        Ok(quote! {
                            pub fn #field_name(&self, ctx: &impl stdlib::ReadContext) -> Option<#inner_ty> {
                                let base_path = #base_path;
                                if ctx.__is_void(&base_path) {
                                    None
                                } else {
                                    ctx.__get(base_path)
                                }
                            }
                        })
                    } else {
                        let inner_wrapper_ty = get_wrapper_ident(&inner_ty, field.span())?;
                        Ok(quote! {
                            pub fn #field_name(&self, ctx: &impl stdlib::ReadContext) -> Option<#inner_wrapper_ty> {
                                let base_path = #base_path;
                                if ctx.__is_void(&base_path) {
                                    None
                                } else {
                                    Some(#inner_wrapper_ty::new(ctx, base_path))
                                }
                            }
                        })
                    }
                } else if is_primitive_type(field_ty) {
                    Ok(quote! {
                        pub fn #field_name(&self, ctx: &impl stdlib::ReadContext) -> #field_ty {
                            ctx.__get(self.base_path.push(#field_name_str)).unwrap()
                        }
                    })
                } else {
                    let field_wrapper_ty = get_wrapper_ident(field_ty, field.span())?;
                    Ok(quote! {
                        pub fn #field_name(&self, ctx: &impl stdlib::ReadContext) -> #field_wrapper_ty {
                            #field_wrapper_ty::new(ctx, self.base_path.push(#field_name_str))
                        }
                    })
                }
            }).collect::<syn::Result<Vec<_>>>()?;

            let setters = fields.named.iter().map(|field| {
                let field_name = field.ident.as_ref().unwrap();
                let field_name_str = field_name.to_string();
                let field_ty = &field.ty;
                let set_field_name = Ident::new(&format!("set_{}", field_name), field_name.span());

                if is_map_type(field_ty) {
                    Ok(quote! { })
                } else if is_option_type(field_ty) {
                    let inner_ty = get_option_inner_type(field_ty)?;
                    Ok(quote! {
                        pub fn #set_field_name(&self, ctx: &impl stdlib::WriteContext, value: Option<#inner_ty>) {
                            let base_path = self.base_path.push(#field_name_str);
                            match value {
                                Some(inner) => ctx.__set(base_path, inner),
                                None => ctx.__set(base_path, ()),
                            }
                        }
                    })
                } else {
                    Ok(quote! {
                        pub fn #set_field_name(&self, ctx: &impl stdlib::WriteContext, value: #field_ty) {
                            ctx.__set(self.base_path.push(#field_name_str), value);
                        }
                    })
                }
            }).collect::<Result::<Vec<_>>>()?;

            let load_fields = fields
                .named
                .iter()
                .map(|field| {
                    let field_name = field.ident.as_ref().unwrap();
                    let _field_name_str = field_name.to_string();
                    let field_ty = &field.ty;

                    if is_map_type(field_ty) {
                        let (_k_ty, _v_ty) = get_map_types(field_ty)?;
                        Ok(quote! {
                            #field_name: self.#field_name().load(ctx)
                        })
                    } else if is_option_type(field_ty) {
                        let inner_ty = get_option_inner_type(field_ty)?;
                        if is_primitive_type(&inner_ty) {
                            Ok(quote! {
                                #field_name: self.#field_name(ctx)
                            })
                        } else {
                            Ok(quote! {
                                #field_name: self.#field_name(ctx).map(|p| p.load(ctx))
                            })
                        }
                    } else if is_primitive_type(field_ty) {
                        Ok(quote! {
                            #field_name: self.#field_name(ctx)
                        })
                    } else {
                        Ok(quote! {
                            #field_name: self.#field_name(ctx).load(ctx)
                        })
                    }
                })
                .collect::<syn::Result<Vec<_>>>()?;

            let result = quote! {
                #[derive(Clone)]
                pub struct #wrapper_name {
                    pub base_path: stdlib::DotPathBuf,
                }

                #[allow(dead_code)]
                impl #wrapper_name {
                    pub fn new(_: &impl stdlib::ReadContext, base_path: stdlib::DotPathBuf) -> Self {
                        Self { base_path }
                    }

                    #(#getters)*

                    #(#setters)*

                    pub fn load(&self, ctx: &impl stdlib::ReadContext) -> #type_name {
                        #type_name {
                            #(#load_fields,)*
                        }
                    }
                }

                #(#special_wrappers)*
            };

            Ok(result)
        }
        _ => Err(Error::new(
            type_name.span(),
            "Wrapper derive only supports structs with named fields",
        )),
    }
}

fn is_map_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .map(|segment| segment.ident == "Map")
            .unwrap_or(false)
    } else {
        false
    }
}

fn get_map_types(ty: &Type) -> syn::Result<(Type, Type)> {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
        && segment.ident == "Map"
        && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
        && args.args.len() == 2
        && let (syn::GenericArgument::Type(k_ty), syn::GenericArgument::Type(v_ty)) =
            (&args.args[0], &args.args[1])
    {
        return Ok((k_ty.clone(), v_ty.clone()));
    }
    Err(Error::new(ty.span(), "Expected Map<K, V> type"))
}

fn is_primitive_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last().map(|s| s.ident.to_string());
        matches!(segment.as_deref(), Some("u64" | "i64" | "String"))
    } else {
        false
    }
}

fn generate_enum_wrapper(
    data_enum: &DataEnum,
    type_name: &Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    let wrapper_name = Ident::new(&format!("{}Wrapper", type_name), type_name.span());

    let wrapper_variants: syn::Result<Vec<_>> = data_enum
        .variants
        .iter()
        .map(|variant| {
            let variant_ident = &variant.ident;
            match &variant.fields {
                Fields::Unit => Ok(quote! { #variant_ident }),
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    let inner_ty = &fields.unnamed[0].ty;
                    let inner_wrapper_ty = get_wrapper_ident(inner_ty, variant.ident.span())?;
                    Ok(quote! { #variant_ident(#inner_wrapper_ty) })
                }
                _ => Err(Error::new(
                    variant.ident.span(),
                    "Wrapper derive only supports unit or single-field tuple variants",
                )),
            }
        })
        .collect();

    let wrapper_variants = wrapper_variants?;

    let variant_names = data_enum
        .variants
        .iter()
        .map(|variant| {
            let variant_name = variant.ident.to_string().to_lowercase();
            quote! { #variant_name }
        })
        .collect::<Vec<_>>();

    let new_arms = data_enum.variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        let variant_name = variant_ident.to_string().to_lowercase();

        Ok(match &variant.fields {
            Fields::Unit => quote! {
                p if p.starts_with(base_path.push(#variant_name).as_ref()) => #wrapper_name::#variant_ident
            },
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let inner_wrapper_ty = get_wrapper_ident(&fields.unnamed[0].ty, variant.ident.span())?;
                quote! {
                    p if p.starts_with(base_path.push(#variant_name).as_ref()) => #wrapper_name::#variant_ident(#inner_wrapper_ty::new(ctx, base_path.push(#variant_name)))
                }
            }
            _ => unreachable!(),
        })
    }).collect::<syn::Result<Vec<_>>>()?;

    let load_arms = data_enum.variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        match &variant.fields {
            Fields::Unit => quote! {
                #wrapper_name::#variant_ident => #type_name::#variant_ident
            },
            Fields::Unnamed(_) => quote! {
                #wrapper_name::#variant_ident(inner_wrapper) => #type_name::#variant_ident(inner_wrapper.load(ctx))
            },
            _ => unreachable!(),
        }
    }).collect::<Vec<_>>();

    Ok(quote! {
        #[derive(Clone)]
        pub enum #wrapper_name {
            #(#wrapper_variants,)*
        }

        impl #wrapper_name {
            pub fn new(ctx: &impl stdlib::ReadContext, base_path: stdlib::DotPathBuf) -> Self {
                ctx.__matching_path(&format!(r"^{}.({})(\..*|$)", base_path, [#(#variant_names),*].join("|")))
                    .map(|path| match path {
                        #(#new_arms,)*
                        _ => {
                            panic!("Matching path not found")
                        }
                    })
                    .unwrap()
            }

            pub fn load(&self, ctx: &impl stdlib::ReadContext) -> #type_name {
                match self {
                    #(#load_arms,)*
                }
            }
        }
    })
}

fn get_wrapper_ident(ty: &Type, span: proc_macro2::Span) -> syn::Result<Ident> {
    if let Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .map(|segment| Ident::new(&format!("{}Wrapper", segment.ident), span))
            .ok_or_else(|| Error::new(span, "Expected a named type for variant field"))
    } else {
        Err(Error::new(span, "Expected a named type for variant field"))
    }
}

fn get_option_inner_type(ty: &Type) -> syn::Result<Type> {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
        && segment.ident == "Option"
        && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
        && args.args.len() == 1
        && let syn::GenericArgument::Type(inner_ty) = &args.args[0]
    {
        return Ok(inner_ty.clone());
    }
    Err(Error::new(ty.span(), "Expected Option<T> type"))
}

#[proc_macro_derive(Root)]
pub fn derive_root(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;

    let body = match &input.data {
        syn::Data::Struct(data_struct) => generate_root_struct(data_struct, name),
        _ => Err(Error::new(
            name.span(),
            "Root derive only supports structs with named fields",
        )),
    };

    let body = match body {
        Ok(body) => body,
        Err(err) => return err.to_compile_error().into(),
    };

    let (_impl_generics, _ty_generics, _where_clause) = generics.split_for_impl();
    let expanded = quote! {
        #body
    };

    TokenStream::from(expanded)
}

fn generate_root_struct(
    data_struct: &DataStruct,
    type_name: &Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    match &data_struct.fields {
        Fields::Named(_) => {
            let wrapper_name = Ident::new(&format!("{}Wrapper", type_name), type_name.span());
            Ok(quote! {
                impl #type_name {
                    pub fn init(self, ctx: &impl stdlib::WriteContext) {
                        ctx.__set(stdlib::DotPathBuf::new(), self)
                    }
                }

                pub fn storage(ctx: &impl stdlib::ReadContext) -> #wrapper_name {
                    #wrapper_name::new(ctx, stdlib::DotPathBuf::new())
                }
            })
        }
        _ => Err(Error::new(
            type_name.span(),
            "Root derive only supports structs with named fields",
        )),
    }
}

#[derive(FromMeta)]
struct ContractConfig {
    name: String,
    world: Option<String>,
    path: Option<String>,
}

#[proc_macro]
pub fn contract(input: TokenStream) -> TokenStream {
    let attr_args = NestedMeta::parse_meta_list(input.into()).unwrap();
    let config = ContractConfig::from_list(&attr_args).unwrap();

    let world = config.world.unwrap_or("contract".to_string());
    let path = config.path.unwrap_or("wit".to_string());
    let name = Ident::from_string(&to_pascal_case(&config.name)).unwrap();
    let boilerplate = quote! {
        use stdlib::*;

        wit_bindgen::generate!({
            world: #world,
            path: #path,
            generate_all,
            additional_derives: [stdlib::Store, stdlib::Wrapper],
        });

        use kontor::built_in::*;
        use kontor::built_in::foreign::ContractAddressWrapper;

        impl ReadContext for context::ViewContext {
            fn __get_str(&self, path: &str) -> Option<String> {
                self.get_str(path)
            }

            fn __get_u64(&self, path: &str) -> Option<u64> {
                self.get_u64(path)
            }

            fn __get_s64(&self, path: &str) -> Option<i64> {
                self.get_s64(path)
            }

            fn __exists(&self, path: &str) -> bool {
                self.exists(path)
            }

            fn __is_void(&self, path: &str) -> bool {
                self.is_void(path)
            }

            fn __matching_path(&self, regexp: &str) -> Option<String> {
                self.matching_path(regexp)
            }

            fn __get<T: Retrieve>(&self, path: DotPathBuf) -> Option<T> {
                T::__get(self, path)
            }
        }

        impl ReadContext for context::ProcContext {
            fn __get_str(&self, path: &str) -> Option<String> {
                self.get_str(path)
            }

            fn __get_u64(&self, path: &str) -> Option<u64> {
                self.get_u64(path)
            }

            fn __get_s64(&self, path: &str) -> Option<i64> {
                self.get_s64(path)
            }

            fn __exists(&self, path: &str) -> bool {
                self.exists(path)
            }

            fn __is_void(&self, path: &str) -> bool {
                self.is_void(path)
            }

            fn __matching_path(&self, regexp: &str) -> Option<String> {
                self.matching_path(regexp)
            }

            fn __get<T: Retrieve>(&self, path: DotPathBuf) -> Option<T> {
                T::__get(self, path)
            }
        }

        impl WriteContext for context::ProcContext {
            fn __set_str(&self, path: &str, value: &str) {
                self.set_str(path, value)
            }

            fn __set_u64(&self, path: &str, value: u64) {
                self.set_u64(path, value)
            }

            fn __set_s64(&self, path: &str, value: i64) {
                self.set_s64(path, value)
            }

            fn __set_void(&self, path: &str) {
                self.set_void(path)
            }

            fn __set<T: stdlib::Store>(&self, path: DotPathBuf, value: T) {
                T::__set(self, path, value)
            }
        }

        impl ReadWriteContext for context::ProcContext {}

        struct #name;
    };

    boilerplate.into()
}

#[derive(FromMeta)]
struct ImportConfig {
    name: String,
    height: i64,
    tx_index: i64,
    path: String,
    world: Option<String>,
}

fn type_name(
    resolve: &wit_parser::Resolve,
    ty: &wit_parser::Type,
) -> anyhow::Result<proc_macro2::TokenStream> {
    match ty {
        wit_parser::Type::U64 => Ok(quote! { u64 }),
        wit_parser::Type::S64 => Ok(quote! { i64 }),
        wit_parser::Type::String => Ok(quote! { String }),
        wit_parser::Type::Id(id) => {
            let ty_def = &resolve.types[*id];
            let name = ty_def
                .name
                .as_ref()
                .ok_or_else(|| anyhow!("Unnamed types are not supported"))?
                .to_upper_camel_case();
            let ident = Ident::new(&name, Span::call_site());
            Ok(quote! { #ident })
        }
        _ => bail!("Unsupported WIT type: {:?}", ty),
    }
}

fn print_typedef_record(
    resolve: &wit_parser::Resolve,
    name: &str,
    record: &wit_parser::Record,
) -> anyhow::Result<proc_macro2::TokenStream> {
    let struct_name = Ident::new(&name.to_upper_camel_case(), Span::call_site());
    let fields = record
        .fields
        .iter()
        .map(|field| {
            let field_name = Ident::new(&field.name.to_snake_case(), Span::call_site());
            let field_ty = type_name(resolve, &field.ty)?;
            Ok(quote! { pub #field_name: #field_ty })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(quote! {
        #[derive(Debug, Clone)]
        pub struct #struct_name {
            #(#fields),*
        }
    })
}

fn print_typedef_enum(
    name: &str,
    enum_: &wit_parser::Enum,
) -> anyhow::Result<proc_macro2::TokenStream> {
    let enum_name = Ident::new(&name.to_upper_camel_case(), Span::call_site());
    let variants = enum_.cases.iter().map(|case| {
        let variant_name = Ident::new(&case.name.to_upper_camel_case(), Span::call_site());
        quote! { #variant_name }
    });

    Ok(quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum #enum_name {
            #(#variants),*
        }
    })
}

fn print_typedef_variant(
    resolve: &wit_parser::Resolve,
    name: &str,
    variant: &wit_parser::Variant,
) -> anyhow::Result<proc_macro2::TokenStream> {
    let enum_name = Ident::new(&name.to_upper_camel_case(), Span::call_site());
    let variants = variant
        .cases
        .iter()
        .map(|case| {
            let variant_name = Ident::new(&case.name.to_upper_camel_case(), Span::call_site());
            match &case.ty {
                Some(ty) => {
                    let ty_name = type_name(resolve, ty)?;
                    Ok(quote! { #variant_name(#ty_name) })
                }
                None => Ok(quote! { #variant_name }),
            }
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(quote! {
        #[derive(Debug, Clone)]
        pub enum #enum_name {
            #(#variants),*
        }
    })
}

#[proc_macro]
pub fn import(input: TokenStream) -> TokenStream {
    let attr_args = NestedMeta::parse_meta_list(input.clone().into()).unwrap();
    let config = ImportConfig::from_list(&attr_args).unwrap();

    let name = config.name;
    let module_name = Ident::from_string(format!("{}_next", name).as_str()).unwrap();
    let height = config.height;
    let tx_index = config.tx_index;
    let world = config.world.unwrap_or("contract".to_string());
    let path = config.path;

    assert!(fs::metadata(&path).is_ok());
    let mut resolve = Resolve::new();
    resolve.push_dir(&path).unwrap();

    let (world_id, world) = resolve
        .worlds
        .iter()
        .find(|(_, w)| w.name == "contract")
        .unwrap();

    let exports = world
        .exports
        .iter()
        .filter_map(|e| match e {
            (WorldKey::Name(name), WorldItem::Function(f))
                if !["init"].contains(&name.as_str()) =>
            {
                Some(f)
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    for export in exports {
        for param in export.params.iter().skip(1) {
            if let (type_name, wit_parser::Type::Id(type_id)) = param {
                let t = resolve.types.get(*type_id).unwrap();
                eprintln!("{} param: {}: {:#?}", export.name, type_name, t);
            }
        }
    }

    let mut type_streams = Vec::new();
    for (id, def) in resolve.types.iter().filter(|(_, def)| {
        if let Some(name) = def.name.as_deref() {
            ![
                "contract-address",
                "view-context",
                "fall-context",
                "proc-context",
                "signer",
            ]
            .contains(&name)
        } else {
            false
        }
    }) {
        let name = def.name.as_deref().expect("Filtered types have names");
        let stream = match &def.kind {
            TypeDefKind::Record(record) => print_typedef_record(&resolve, name, record),
            TypeDefKind::Enum(enum_) => print_typedef_enum(name, enum_),
            TypeDefKind::Variant(variant) => print_typedef_variant(&resolve, name, variant),
            _ => panic!("Unsupported type definition kind: {:?}", def.kind),
        }
        .expect("Failed to generate type");
        type_streams.push(stream);
    }

    quote! {
        mod #module_name {
            use wasm_wave::wasm::WasmValue as _;

            use super::context;
            use super::foreign;

            const CONTRACT_NAME: &str = #name;

            impl foreign::ContractAddress {
                pub fn wave_type() -> wasm_wave::value::Type {
                    wasm_wave::value::Type::record([
                        ("name", wasm_wave::value::Type::STRING),
                        ("height", wasm_wave::value::Type::S64),
                        ("tx_index", wasm_wave::value::Type::S64),
                    ])
                    .unwrap()
                }
            }

            impl From<foreign::ContractAddress> for wasm_wave::value::Value {
                fn from(value_: foreign::ContractAddress) -> Self {
                    wasm_wave::value::Value::make_record(
                        &foreign::ContractAddress::wave_type(),
                        [
                            ("name", wasm_wave::value::Value::from(value_.name)),
                            ("height", wasm_wave::value::Value::from(value_.height)),
                            ("tx_index", wasm_wave::value::Value::from(value_.tx_index)),
                        ],
                    )
                    .unwrap()
                }
            }

            impl From<wasm_wave::value::Value> for foreign::ContractAddress {
                fn from(value_: wasm_wave::value::Value) -> Self {
                    let mut name = None;
                    let mut height = None;
                    let mut tx_index = None;

                    for (key_, val_) in  value_.unwrap_record() {
                        match key_.as_ref() {
                            "name" => name = Some(val_.unwrap_string().into_owned()),
                            "height" => height = Some(val_.unwrap_s64()),
                            "tx_index" => tx_index = Some(val_.unwrap_s64()),
                            key_ => panic!("Unknown field: {key_}"),
                        }
                    }

                    Self {
                        name: name.expect("Missing 'name' field"),
                        height: height.expect("Missing 'height' field"),
                        tx_index: tx_index.expect("Missing 'tx_index' field"),
                    }
                }
            }

             #(#type_streams)*
        }
    }
    .into()
}

#[proc_macro_derive(Wavey)]
pub fn derive_wave_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let wave_type_body = match &input.data {
        syn::Data::Struct(data) => generate_struct_wave_type(data),
        syn::Data::Enum(data) => generate_enum_wave_type(data),
        _ => Err(Error::new(
            name.span(),
            "Wavey derive is only supported for structs and enums",
        )),
    };

    let wave_type_body = match wave_type_body {
        Ok(body) => body,
        Err(err) => return err.to_compile_error().into(),
    };

    let from_self_body = match &input.data {
        syn::Data::Struct(data) => generate_struct_to_value(data, name),
        syn::Data::Enum(data) => generate_enum_to_value(data, name),
        _ => Err(Error::new(
            name.span(),
            "Wavey derive is only supported for structs and enums",
        )),
    };

    let from_self_body = match from_self_body {
        Ok(body) => body,
        Err(err) => return err.to_compile_error().into(),
    };

    let from_value_body = match &input.data {
        syn::Data::Struct(data) => generate_struct_from_value(data, name),
        syn::Data::Enum(data) => generate_enum_from_value(data, name),
        _ => Err(Error::new(
            name.span(),
            "Wavey derive is only supported for structs and enums",
        )),
    };

    let from_value_body = match from_value_body {
        Ok(body) => body,
        Err(err) => return err.to_compile_error().into(),
    };

    quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub fn wave_type() -> wasm_wave::value::Type {
                #wave_type_body
            }
        }

        impl #impl_generics From<#name #ty_generics> for wasm_wave::value::Value #where_clause {
            fn from(value_: #name #ty_generics) -> Self {
                #from_self_body
            }
        }

        impl #impl_generics From<wasm_wave::value::Value> for #name #ty_generics #where_clause {
            fn from(value_: wasm_wave::value::Value) -> Self {
                #from_value_body
            }
        }
    }
    .into()
}

fn generate_struct_wave_type(data: &DataStruct) -> syn::Result<proc_macro2::TokenStream> {
    match &data.fields {
        Fields::Named(fields) => {
            let field_types = fields
                .named
                .iter()
                .map(|field| {
                    let field_name_str = field.ident.as_ref().unwrap().to_string();
                    let field_ty = &field.ty;
                    let wave_ty = type_to_wave_type(field_ty)?;
                    Ok(quote! { (#field_name_str, #wave_ty) })
                })
                .collect::<syn::Result<Vec<_>>>()?;
            Ok(quote! {
                wasm_wave::value::Type::record([#(#field_types),*]).unwrap()
            })
        }
        _ => Err(Error::new(
            data.struct_token.span,
            "Wavey derive only supports structs with named fields",
        )),
    }
}

fn generate_enum_wave_type(data: &DataEnum) -> syn::Result<proc_macro2::TokenStream> {
    let variant_types = data
        .variants
        .iter()
        .map(|variant| {
            let variant_name = variant.ident.to_string().to_lowercase();
            match &variant.fields {
                Fields::Unit => Ok(quote! { (#variant_name, None) }),
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    let inner_ty = &fields.unnamed[0].ty;
                    let inner_wave_ty = type_to_wave_type(inner_ty)?;
                    Ok(quote! { (#variant_name, Some(#inner_wave_ty)) })
                }
                _ => Err(Error::new(
                    variant.span(),
                    "Wavey derive only supports unit or single-field tuple variants for enums",
                )),
            }
        })
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(quote! {
        wasm_wave::value::Type::variant([#(#variant_types),*]).unwrap()
    })
}

fn generate_struct_to_value(
    data: &DataStruct,
    name: &Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    match &data.fields {
        Fields::Named(fields) => {
            let field_assigns = fields.named.iter().map(|field| {
                let field_name = field.ident.as_ref().unwrap();
                let field_name_str = field_name.to_string();
                quote! { (#field_name_str, wasm_wave::value::Value::from(value_.#field_name)) }
            });
            Ok(quote! {
                wasm_wave::value::Value::make_record(
                    &#name::wave_type(),
                    [#(#field_assigns),*],
                ).unwrap()
            })
        }
        _ => Err(Error::new(
            data.struct_token.span,
            "Wavey derive only supports structs with named fields",
        )),
    }
}

fn generate_enum_to_value(data: &DataEnum, name: &Ident) -> syn::Result<proc_macro2::TokenStream> {
    let arms = data.variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        let variant_name = variant_ident.to_string().to_lowercase();
        match &variant.fields {
            Fields::Unit => Ok(quote! {
                #name::#variant_ident => wasm_wave::value::Value::make_variant(&#name::wave_type(), #variant_name, None)
            }),
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => Ok(quote! {
                #name::#variant_ident(operand) => wasm_wave::value::Value::make_variant(&#name::wave_type(), #variant_name, Some(wasm_wave::value::Value::from(operand)))
            }),
            _ => Err(Error::new(variant.span(), "Wavey derive only supports unit or single-field tuple variants for enums")),
        }
    }).collect::<syn::Result<Vec<_>>>()?;
    Ok(quote! {
        (match value_ {
            #(#arms,)*
        }).unwrap()
    })
}

fn generate_struct_from_value(
    data: &DataStruct,
    name: &Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    match &data.fields {
        Fields::Named(fields) => {
            let mut_inits = fields.named.iter().map(|field| {
                let field_name = field.ident.as_ref().unwrap();
                quote! { let mut #field_name = None; }
            });
            let match_arms = fields.named.iter().map(|field| {
                let field_name = field.ident.as_ref().unwrap();
                let field_name_str = field_name.to_string();
                let unwrap_expr = unwrap_expr_for_type(&field.ty)
                    .unwrap_or_else(|_| panic!("Could not unwrap expr for type: {:?}", &field.ty));
                quote! { #field_name_str => #field_name = Some(val_.#unwrap_expr), }
            });
            let constructs = fields.named.iter().map(|field| {
                let field_name = field.ident.as_ref().unwrap();
                let field_name_str = field_name.to_string();
                quote! { #field_name: #field_name.expect(format!("Missing '{}' field"), #field_name_str), }
            });
            Ok(quote! {
                #(#mut_inits)*
                for (key_, val_) in value_.unwrap_record() {
                    match key_.as_ref() {
                        #(#match_arms)*
                        key_ => panic!("Unknown field: {key_}"),
                    }
                }
                #name {
                    #(#constructs)*
                }
            })
        }
        _ => Err(Error::new(
            data.struct_token.span,
            "Wavey derive only supports structs with named fields",
        )),
    }
}

fn generate_enum_from_value(
    data: &DataEnum,
    name: &Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    let arms = data.variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        let variant_name = variant_ident.to_string().to_lowercase();
        match &variant.fields {
            Fields::Unit => Ok(quote! {
                key_ if key_.eq(#variant_name) => #name::#variant_ident,
            }),
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let unwrap_expr = unwrap_expr_for_type(&fields.unnamed[0].ty)?;
                Ok(quote! {
                    key_ if key_.eq(#variant_name) => #name::#variant_ident(val_.unwrap().#unwrap_expr),
                })
            }
            _ => Err(Error::new(variant.span(), "Wavey derive only supports unit or single-field tuple variants for enums")),
        }
    }).collect::<syn::Result<Vec<_>>>()?;
    Ok(quote! {
        let (key_, val_) = value_.unwrap_variant();
        match key_ {
            #(#arms)*
            key_ => panic!("Unknown tag {key_}"),
        }
    })
}

fn type_to_wave_type(ty: &Type) -> syn::Result<proc_macro2::TokenStream> {
    if let Some(prim) = get_wave_primitive_type(ty) {
        Ok(prim)
    } else {
        Ok(quote! { #ty::wave_type() })
    }
}

fn get_wave_primitive_type(ty: &Type) -> Option<proc_macro2::TokenStream> {
    if let Type::Path(TypePath { qself: None, path }) = ty
        && path.segments.len() == 1
    {
        let segment = &path.segments[0];
        if segment.arguments == PathArguments::None {
            match segment.ident.to_string().as_str() {
                "u64" => Some(quote! { wasm_wave::value::Type::U64 }),
                "i64" => Some(quote! { wasm_wave::value::Type::S64 }),
                "String" => Some(quote! { wasm_wave::value::Type::STRING }),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    }
}

fn unwrap_expr_for_type(ty: &Type) -> syn::Result<proc_macro2::TokenStream> {
    if let Type::Path(TypePath { qself: None, path }) = ty
        && path.segments.len() == 1
    {
        let segment = &path.segments[0];
        if segment.arguments == PathArguments::None {
            let ident = segment.ident.to_string();
            match ident.as_str() {
                "u64" => return Ok(quote! { unwrap_u64() }),
                "i64" => return Ok(quote! { unwrap_s64() }),
                "String" => return Ok(quote! { unwrap_string().into_owned() }),
                _ => {}
            }
        }
    }
    Ok(quote! { into_owned().into() })
}
