//! Relationship metadata for SQLModel Rust.
//!
//! Relationships are defined at compile-time (via derive macros) and represented
//! as static metadata on each `Model`. This allows higher-level layers (query
//! builder, session/UoW, eager/lazy loaders) to generate correct SQL and load
//! related objects without runtime reflection.

use crate::{Model, Value};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::sync::OnceLock;

/// The type of relationship between two models.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RelationshipKind {
    /// One-to-one: `Hero` has one `Profile`.
    OneToOne,
    /// Many-to-one: many `Hero`s belong to one `Team`.
    #[default]
    ManyToOne,
    /// One-to-many: one `Team` has many `Hero`s.
    OneToMany,
    /// Many-to-many: `Hero`s have many `Power`s via a link table.
    ManyToMany,
}

/// Information about a link/join table for many-to-many relationships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkTableInfo {
    /// The link table name (e.g., `"hero_powers"`).
    pub table_name: &'static str,

    /// Column in link table pointing to the local model (e.g., `"hero_id"`).
    pub local_column: &'static str,

    /// Column in link table pointing to the remote model (e.g., `"power_id"`).
    pub remote_column: &'static str,
}

impl LinkTableInfo {
    /// Create a new link-table definition.
    #[must_use]
    pub const fn new(
        table_name: &'static str,
        local_column: &'static str,
        remote_column: &'static str,
    ) -> Self {
        Self {
            table_name,
            local_column,
            remote_column,
        }
    }
}

/// Metadata about a relationship between models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationshipInfo {
    /// Name of the relationship field.
    pub name: &'static str,

    /// The related model's table name.
    pub related_table: &'static str,

    /// Kind of relationship.
    pub kind: RelationshipKind,

    /// Local foreign key column (for ManyToOne).
    /// e.g., `"team_id"` on `Hero`.
    pub local_key: Option<&'static str>,

    /// Remote foreign key column (for OneToMany).
    /// e.g., `"team_id"` on `Hero` when accessed from `Team`.
    pub remote_key: Option<&'static str>,

    /// Link table for ManyToMany relationships.
    pub link_table: Option<LinkTableInfo>,

    /// The field on the related model that points back.
    pub back_populates: Option<&'static str>,

    /// Whether to use lazy loading.
    pub lazy: bool,

    /// Cascade delete behavior.
    pub cascade_delete: bool,
}

impl RelationshipInfo {
    /// Create a new relationship with required fields.
    #[must_use]
    pub const fn new(
        name: &'static str,
        related_table: &'static str,
        kind: RelationshipKind,
    ) -> Self {
        Self {
            name,
            related_table,
            kind,
            local_key: None,
            remote_key: None,
            link_table: None,
            back_populates: None,
            lazy: false,
            cascade_delete: false,
        }
    }

    /// Set the local foreign key column (ManyToOne).
    #[must_use]
    pub const fn local_key(mut self, key: &'static str) -> Self {
        self.local_key = Some(key);
        self
    }

    /// Set the remote foreign key column (OneToMany).
    #[must_use]
    pub const fn remote_key(mut self, key: &'static str) -> Self {
        self.remote_key = Some(key);
        self
    }

    /// Set the link table metadata (ManyToMany).
    #[must_use]
    pub const fn link_table(mut self, info: LinkTableInfo) -> Self {
        self.link_table = Some(info);
        self
    }

    /// Set the back-populates field name (bidirectional relationships).
    #[must_use]
    pub const fn back_populates(mut self, field: &'static str) -> Self {
        self.back_populates = Some(field);
        self
    }

    /// Enable/disable lazy loading.
    #[must_use]
    pub const fn lazy(mut self, value: bool) -> Self {
        self.lazy = value;
        self
    }

    /// Enable/disable cascade delete behavior.
    #[must_use]
    pub const fn cascade_delete(mut self, value: bool) -> Self {
        self.cascade_delete = value;
        self
    }
}

impl Default for RelationshipInfo {
    fn default() -> Self {
        Self::new("", "", RelationshipKind::default())
    }
}

/// A related single object (many-to-one or one-to-one).
///
/// This wrapper can be in one of three states:
/// - **Empty**: no relationship (`fk_value` is None)
/// - **Unloaded**: has FK value but not fetched yet (`fk_value` is Some, `loaded` unset)
/// - **Loaded**: the object has been fetched and cached (`loaded` set)
pub struct Related<T: Model> {
    fk_value: Option<Value>,
    loaded: OnceLock<Option<T>>,
}

impl<T: Model> Related<T> {
    /// Create an empty relationship (null FK, not loaded).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            fk_value: None,
            loaded: OnceLock::new(),
        }
    }

    /// Create from a foreign key value (not yet loaded).
    #[must_use]
    pub fn from_fk(fk: impl Into<Value>) -> Self {
        Self {
            fk_value: Some(fk.into()),
            loaded: OnceLock::new(),
        }
    }

    /// Create with an already-loaded object.
    #[must_use]
    pub fn loaded(obj: T) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(Some(obj));
        Self {
            fk_value: None,
            loaded: cell,
        }
    }

    /// Get the loaded object (None if not loaded or loaded as null).
    #[must_use]
    pub fn get(&self) -> Option<&T> {
        self.loaded.get().and_then(|o| o.as_ref())
    }

    /// Check if the relationship has been loaded (including loaded-null).
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.loaded.get().is_some()
    }

    /// Check if the relationship is empty (null FK).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fk_value.is_none()
    }

    /// Get the foreign key value (if present).
    #[must_use]
    pub fn fk(&self) -> Option<&Value> {
        self.fk_value.as_ref()
    }

    /// Set the loaded object (internal use by query system).
    pub fn set_loaded(&self, obj: Option<T>) -> Result<(), Option<T>> {
        self.loaded.set(obj)
    }
}

impl<T: Model> Default for Related<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T: Model + Clone> Clone for Related<T> {
    fn clone(&self) -> Self {
        let cloned = Self {
            fk_value: self.fk_value.clone(),
            loaded: OnceLock::new(),
        };

        if let Some(value) = self.loaded.get() {
            let _ = cloned.loaded.set(value.clone());
        }

        cloned
    }
}

impl<T: Model + fmt::Debug> fmt::Debug for Related<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = if self.is_loaded() {
            "loaded"
        } else if self.is_empty() {
            "empty"
        } else {
            "unloaded"
        };

        f.debug_struct("Related")
            .field("state", &state)
            .field("fk_value", &self.fk_value)
            .field("loaded", &self.get())
            .finish()
    }
}

impl<T> Serialize for Related<T>
where
    T: Model + Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.loaded.get() {
            Some(Some(obj)) => obj.serialize(serializer),
            Some(None) | None => serializer.serialize_none(),
        }
    }
}

impl<'de, T> Deserialize<'de> for Related<T>
where
    T: Model + Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let opt = Option::<T>::deserialize(deserializer)?;
        Ok(match opt {
            Some(obj) => Self::loaded(obj),
            None => Self::empty(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FieldInfo, Result, Row};
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_relationship_kind_default() {
        assert_eq!(RelationshipKind::default(), RelationshipKind::ManyToOne);
    }

    #[test]
    fn test_relationship_info_builder_chain() {
        let info = RelationshipInfo::new("team", "teams", RelationshipKind::ManyToOne)
            .local_key("team_id")
            .back_populates("heroes")
            .lazy(true)
            .cascade_delete(true);

        assert_eq!(info.name, "team");
        assert_eq!(info.related_table, "teams");
        assert_eq!(info.kind, RelationshipKind::ManyToOne);
        assert_eq!(info.local_key, Some("team_id"));
        assert_eq!(info.remote_key, None);
        assert_eq!(info.link_table, None);
        assert_eq!(info.back_populates, Some("heroes"));
        assert!(info.lazy);
        assert!(info.cascade_delete);
    }

    #[test]
    fn test_link_table_info_new() {
        let link = LinkTableInfo::new("hero_powers", "hero_id", "power_id");
        assert_eq!(link.table_name, "hero_powers");
        assert_eq!(link.local_column, "hero_id");
        assert_eq!(link.remote_column, "power_id");
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Team {
        id: Option<i64>,
        name: String,
    }

    impl Model for Team {
        const TABLE_NAME: &'static str = "teams";
        const PRIMARY_KEY: &'static [&'static str] = &["id"];

        fn fields() -> &'static [FieldInfo] {
            &[]
        }

        fn to_row(&self) -> Vec<(&'static str, Value)> {
            vec![]
        }

        fn from_row(_row: &Row) -> Result<Self> {
            Ok(Self {
                id: None,
                name: String::new(),
            })
        }

        fn primary_key_value(&self) -> Vec<Value> {
            vec![]
        }

        fn is_new(&self) -> bool {
            self.id.is_none()
        }
    }

    #[test]
    fn test_related_empty_creates_unloaded_state() {
        let rel = Related::<Team>::empty();
        assert!(rel.is_empty());
        assert!(!rel.is_loaded());
        assert!(rel.get().is_none());
        assert!(rel.fk().is_none());
    }

    #[test]
    fn test_related_from_fk_stores_value() {
        let rel = Related::<Team>::from_fk(42_i64);
        assert!(!rel.is_empty());
        assert_eq!(rel.fk(), Some(&Value::from(42_i64)));
        assert!(!rel.is_loaded());
        assert!(rel.get().is_none());
    }

    #[test]
    fn test_related_loaded_sets_object() {
        let team = Team {
            id: Some(1),
            name: "Avengers".to_string(),
        };
        let rel = Related::loaded(team.clone());
        assert!(rel.is_loaded());
        assert!(rel.fk().is_none());
        assert_eq!(rel.get(), Some(&team));
    }

    #[test]
    fn test_related_set_loaded_succeeds_first_time() {
        let rel = Related::<Team>::from_fk(1_i64);
        let team = Team {
            id: Some(1),
            name: "Avengers".to_string(),
        };
        assert!(rel.set_loaded(Some(team.clone())).is_ok());
        assert!(rel.is_loaded());
        assert_eq!(rel.get(), Some(&team));
    }

    #[test]
    fn test_related_set_loaded_fails_second_time() {
        let rel = Related::<Team>::empty();
        assert!(rel.set_loaded(None).is_ok());
        assert!(rel.is_loaded());
        assert!(rel.set_loaded(None).is_err());
    }

    #[test]
    fn test_related_default_is_empty() {
        let rel: Related<Team> = Related::default();
        assert!(rel.is_empty());
    }

    #[test]
    fn test_related_clone_unloaded_is_unloaded() {
        let rel = Related::<Team>::from_fk(7_i64);
        let cloned = rel.clone();
        assert!(!cloned.is_loaded());
        assert_eq!(cloned.fk(), rel.fk());
    }

    #[test]
    fn test_related_clone_loaded_preserves_object() {
        let team = Team {
            id: Some(1),
            name: "Avengers".to_string(),
        };
        let rel = Related::loaded(team.clone());
        let cloned = rel.clone();
        assert!(cloned.is_loaded());
        assert_eq!(cloned.get(), Some(&team));
    }

    #[test]
    fn test_related_debug_output_shows_state() {
        let rel = Related::<Team>::from_fk(1_i64);
        let s = format!("{rel:?}");
        assert!(s.contains("state"));
        assert!(s.contains("unloaded"));
    }

    #[test]
    fn test_related_serde_serialize_loaded_outputs_object() {
        let rel = Related::loaded(Team {
            id: Some(1),
            name: "Avengers".to_string(),
        });
        let json = serde_json::to_value(&rel).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": 1,
                "name": "Avengers"
            })
        );
    }

    #[test]
    fn test_related_serde_serialize_unloaded_outputs_null() {
        let rel = Related::<Team>::from_fk(1_i64);
        let json = serde_json::to_value(&rel).unwrap();
        assert_eq!(json, serde_json::Value::Null);
    }

    #[test]
    fn test_related_serde_deserialize_object_creates_loaded() {
        let rel: Related<Team> = serde_json::from_value(serde_json::json!({
            "id": 1,
            "name": "Avengers"
        }))
        .unwrap();

        let expected = Team {
            id: Some(1),
            name: "Avengers".to_string(),
        };
        assert!(rel.is_loaded());
        assert_eq!(rel.get(), Some(&expected));
    }

    #[test]
    fn test_related_serde_deserialize_null_creates_empty() {
        let rel: Related<Team> = serde_json::from_value(serde_json::Value::Null).unwrap();
        assert!(rel.is_empty());
        assert!(!rel.is_loaded());
        assert!(rel.get().is_none());
    }

    #[test]
    fn test_related_serde_roundtrip_preserves_data() {
        let rel = Related::loaded(Team {
            id: Some(1),
            name: "Avengers".to_string(),
        });
        let json = serde_json::to_string(&rel).unwrap();
        let decoded: Related<Team> = serde_json::from_str(&json).unwrap();
        assert!(decoded.is_loaded());
        assert_eq!(decoded.get(), rel.get());
    }
}
