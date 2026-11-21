use async_graphql::InputObject;
use sea_orm::sea_query::{Expr, SimpleExpr};

/// How a filter compares a string column against the text it is given
#[derive(InputObject, Clone, Default)]
pub struct StringFilter {
    /// The text to compare the column against
    pub value: String,

    /// Match the whole column exactly and case-sensitively, instead of looking
    /// for `value` anywhere in it
    #[graphql(default)]
    pub exact: bool,
}

impl StringFilter {
    /// The SQL condition this filter stands for on `col`.
    pub fn to_condition(&self, col: Expr) -> SimpleExpr {
        if self.exact {
            // Every string column here has a case-insensitive collation, so an
            // exact match has to spell the case-sensitive comparison out. This
            // stays sargable: MariaDB reads the column's own index to get the
            // rows that match case-insensitively, then rechecks each one under
            // `utf8mb4_bin`.
            Expr::cust_with_exprs(
                "? = ? COLLATE utf8mb4_bin",
                [col.into(), Expr::val(self.value.as_str()).into()],
            )
        } else {
            col.like(format!("%{}%", escape_like(&self.value)))
        }
    }
}

/// Quotes the wildcards in `value`, so that it is looked for as itself rather
/// than as a pattern: searching for `100%` should mean the two digits and the
/// sign, not "starts with 100".
///
/// `\` is what `LIKE` quotes with by default, which makes it a wildcard of
/// sorts too — it has to be quoted with itself.
fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for c in value.chars() {
        if matches!(c, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(c);
    }

    escaped
}

#[cfg(test)]
mod tests {
    use sea_orm::sea_query::{MysqlQueryBuilder, Query, Value};

    use super::{Expr, StringFilter};

    /// The SQL and bound values a filter on `players`.`login` compiles to.
    fn condition_sql(filter: &StringFilter) -> (String, Vec<Value>) {
        let (sql, values) = Query::select()
            .expr(Expr::cust("1"))
            .and_where(filter.to_condition(Expr::col(("p", "login"))))
            .build(MysqlQueryBuilder);
        (sql, values.0)
    }

    #[test]
    fn substring_match_is_a_like() {
        let (sql, values) = condition_sql(&StringFilter {
            value: "smoke".to_owned(),
            exact: false,
        });

        assert_eq!(sql, "SELECT 1 WHERE `p`.`login` LIKE ?");
        assert_eq!(values, [Value::from("%smoke%")]);
    }

    /// Wildcards typed into a filter are part of what the user is looking for,
    /// not pattern syntax.
    #[test]
    fn substring_match_quotes_wildcards() {
        let (_, values) = condition_sql(&StringFilter {
            value: r"100%_\o/".to_owned(),
            exact: false,
        });

        assert_eq!(values, [Value::from(r"%100\%\_\\o/%")]);
    }

    /// Nothing to quote on this side: `=` has no pattern syntax to protect the
    /// value from.
    #[test]
    fn exact_match_keeps_wildcards_verbatim() {
        let (_, values) = condition_sql(&StringFilter {
            value: r"100%_\o/".to_owned(),
            exact: true,
        });

        assert_eq!(values, [Value::from(r"100%_\o/")]);
    }

    /// The comparison has to name a binary collation: the column's own is
    /// case-insensitive, so a plain `=` would still match `SMOKEGUN`.
    #[test]
    fn exact_match_is_case_sensitive() {
        let (sql, values) = condition_sql(&StringFilter {
            value: "smokegun".to_owned(),
            exact: true,
        });

        assert_eq!(sql, "SELECT 1 WHERE `p`.`login` = ? COLLATE utf8mb4_bin");
        assert_eq!(values, [Value::from("smokegun")]);
    }
}
