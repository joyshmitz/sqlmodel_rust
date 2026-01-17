//! Procedural macros for SQLModel Rust.
//!
//! This crate provides derive macros for:
//! - `Model` - ORM-style struct mapping
//! - `Validate` - Field validation
//! - `JsonSchema` - JSON Schema generation (for OpenAPI)

use proc_macro::TokenStream;

/// Derive macro for the `Model` trait.
///
/// This macro generates implementations for:
/// - Table name and primary key metadata
/// - Field information
/// - Row conversion (to_row, from_row)
/// - Primary key access
///
/// # Attributes
///
/// - `#[sqlmodel(table = "name")]` - Override table name (defaults to snake_case struct name)
/// - `#[sqlmodel(primary_key)]` - Mark field as primary key
/// - `#[sqlmodel(auto_increment)]` - Mark field as auto-incrementing
/// - `#[sqlmodel(column = "name")]` - Override column name
/// - `#[sqlmodel(nullable)]` - Mark field as nullable
/// - `#[sqlmodel(unique)]` - Add unique constraint
/// - `#[sqlmodel(default = "expr")]` - Set default SQL expression
/// - `#[sqlmodel(foreign_key = "table.column")]` - Add foreign key reference
/// - `#[sqlmodel(index = "name")]` - Add to named index
/// - `#[sqlmodel(skip)]` - Skip this field in database operations
///
/// # Example
///
/// ```ignore
/// use sqlmodel::Model;
///
/// #[derive(Model)]
/// #[sqlmodel(table = "heroes")]
/// struct Hero {
///     #[sqlmodel(primary_key, auto_increment)]
///     id: Option<i64>,
///
///     #[sqlmodel(unique)]
///     name: String,
///
///     secret_name: String,
///
///     #[sqlmodel(nullable)]
///     age: Option<i32>,
///
///     #[sqlmodel(foreign_key = "teams.id")]
///     team_id: Option<i64>,
/// }
/// ```
#[proc_macro_derive(Model, attributes(sqlmodel))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    // TODO: Implement Model derive macro
    // For now, return empty implementation
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let name = &input.ident;

    let expanded = quote::quote! {
        impl sqlmodel_core::Model for #name {
            const TABLE_NAME: &'static str = stringify!(#name);
            const PRIMARY_KEY: &'static [&'static str] = &["id"];

            fn fields() -> &'static [sqlmodel_core::FieldInfo] {
                &[] // TODO: Generate from struct fields
            }

            fn to_row(&self) -> Vec<(&'static str, sqlmodel_core::Value)> {
                vec![] // TODO: Generate from struct fields
            }

            fn from_row(_row: &sqlmodel_core::Row) -> sqlmodel_core::Result<Self> {
                todo!("Model::from_row not yet implemented")
            }

            fn primary_key_value(&self) -> Vec<sqlmodel_core::Value> {
                vec![] // TODO: Generate from primary key fields
            }

            fn is_new(&self) -> bool {
                true // TODO: Check if primary key is None/default
            }
        }
    };

    TokenStream::from(expanded)
}

/// Derive macro for field validation.
///
/// Generates validation logic based on field attributes.
///
/// # Attributes
///
/// - `#[validate(min = N)]` - Minimum value for numbers
/// - `#[validate(max = N)]` - Maximum value for numbers
/// - `#[validate(min_length = N)]` - Minimum length for strings
/// - `#[validate(max_length = N)]` - Maximum length for strings
/// - `#[validate(pattern = "regex")]` - Regex pattern for strings
/// - `#[validate(email)]` - Email format validation
/// - `#[validate(url)]` - URL format validation
/// - `#[validate(custom = "fn_name")]` - Custom validation function
#[proc_macro_derive(Validate, attributes(validate))]
pub fn derive_validate(input: TokenStream) -> TokenStream {
    // TODO: Implement Validate derive macro
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let _name = &input.ident;

    // Return empty for now
    TokenStream::new()
}

/// Attribute macro for defining SQL functions in handlers.
///
/// # Example
///
/// ```ignore
/// #[sqlmodel::query]
/// async fn get_heroes(cx: &Cx, conn: &impl Connection) -> Vec<Hero> {
///     sqlmodel::select!(Hero).all(cx, conn).await
/// }
/// ```
#[proc_macro_attribute]
pub fn query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // TODO: Implement query attribute macro
    item
}
