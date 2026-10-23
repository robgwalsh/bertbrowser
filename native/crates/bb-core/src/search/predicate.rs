//! A WHERE fragment compiled from a query, plus the values it binds. `complete` is true when the
//! fragment matches *exactly* what `matches` matches; false when some term could not be expressed and
//! the fragment is a *superset*. A caller may only push LIMIT into the query when it is true.
//! Spec: Core/Services/Search/SqlPredicate.cs.

use rusqlite::types::Value;

#[derive(Clone, Debug)]
pub struct SqlPredicate {
    /// The fragment, already parenthesised where it needs to be.
    pub sql: String,
    /// Named parameters (`:f0`, ...) the caller binds; unique per compile.
    pub parameters: Vec<(String, Value)>,
    pub complete: bool,
}

impl SqlPredicate {
    /// The parameters as rusqlite wants them.
    pub fn bindings(&self) -> Vec<(&str, &dyn rusqlite::ToSql)> {
        self.parameters
            .iter()
            .map(|(n, v)| (n.as_str(), v as &dyn rusqlite::ToSql))
            .collect()
    }
}

/// Accumulates a `SqlPredicate` as the node tree writes itself out.
#[derive(Default)]
pub struct SqlPredicateBuilder {
    sql: String,
    parameters: Vec<(String, Value)>,
    incomplete: bool,
}

impl SqlPredicateBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends raw SQL text.
    pub fn append(&mut self, sql: &str) -> &mut Self {
        self.sql.push_str(sql);
        self
    }

    /// Binds a value and appends its parameter name.
    pub fn append_parameter(&mut self, value: impl Into<Value>) -> &mut Self {
        let name = format!(":f{}", self.parameters.len());
        self.sql.push_str(&name);
        self.parameters.push((name, value.into()));
        self
    }

    /// Records that the SQL is now a superset of the real predicate.
    pub fn mark_incomplete(&mut self) {
        self.incomplete = true;
    }

    pub fn is_incomplete(&self) -> bool {
        self.incomplete
    }

    pub fn build(self) -> SqlPredicate {
        SqlPredicate {
            sql: self.sql,
            parameters: self.parameters,
            complete: !self.incomplete,
        }
    }
}
