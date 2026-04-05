use serde_json::Value;

#[derive(Debug)]
pub struct FilterResult {
    pub where_clause: String,
    pub bindings: Vec<(String, Value)>,
}

struct Ctx {
    bindings: Vec<(String, Value)>,
    counter: usize,
}

impl Ctx {
    fn new() -> Self {
        Self { bindings: Vec::new(), counter: 0 }
    }

    fn param(&mut self, value: Value) -> String {
        let name = format!("f{}", self.counter);
        self.counter += 1;
        self.bindings.push((name.clone(), value));
        format!("${name}")
    }
}

pub fn parse_filters(filters: &Value) -> Result<FilterResult, String> {
    let obj = filters.as_object().ok_or("filters must be a JSON object")?;
    if obj.is_empty() {
        return Err("filters object is empty".into());
    }
    let mut ctx = Ctx::new();
    let clause = parse_object(obj, "", &mut ctx)?;
    Ok(FilterResult { where_clause: clause, bindings: ctx.bindings })
}

fn parse_object(
    obj: &serde_json::Map<String, Value>,
    path: &str,
    ctx: &mut Ctx,
) -> Result<String, String> {
    let mut conditions = Vec::new();

    for (key, value) in obj {
        match key.as_str() {
            "$and" => {
                let parts = parse_logical_array(value, path, ctx, "$and")?;
                if !parts.is_empty() {
                    conditions.push(format!("({})", parts.join(" AND ")));
                }
            }
            "$or" => {
                let parts = parse_logical_array(value, path, ctx, "$or")?;
                if !parts.is_empty() {
                    conditions.push(format!("({})", parts.join(" OR ")));
                }
            }
            k if k.starts_with('$') => {
                if path.is_empty() {
                    return Err(format!("operator {k} requires a field path"));
                }
                let sql_op = operator_to_sql(k)?;
                let param = ctx.param(value.clone());
                conditions.push(format!("{path} {sql_op} {param}"));
            }
            field => {
                validate_field_name(field)?;
                let new_path = if path.is_empty() {
                    format!("custom_attributes.{field}")
                } else {
                    format!("{path}.{field}")
                };
                conditions.push(parse_value(value, &new_path, ctx)?);
            }
        }
    }

    if conditions.is_empty() {
        return Err("empty filter condition".into());
    }
    Ok(conditions.join(" AND "))
}

fn parse_value(value: &Value, path: &str, ctx: &mut Ctx) -> Result<String, String> {
    match value {
        Value::Object(inner) => parse_object(inner, path, ctx),
        Value::Null => Ok(format!("{path} IS NONE")),
        _ => {
            let param = ctx.param(value.clone());
            Ok(format!("{path} = {param}"))
        }
    }
}

fn parse_logical_array(
    value: &Value,
    path: &str,
    ctx: &mut Ctx,
    op_name: &str,
) -> Result<Vec<String>, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| format!("{op_name} value must be an array"))?;
    arr.iter()
        .map(|v| {
            let inner = v
                .as_object()
                .ok_or_else(|| format!("{op_name} elements must be objects"))?;
            parse_object(inner, path, ctx)
        })
        .collect()
}

fn operator_to_sql(op: &str) -> Result<&'static str, String> {
    match op {
        "$eq" => Ok("="),
        "$ne" => Ok("!="),
        "$gt" => Ok(">"),
        "$gte" => Ok(">="),
        "$lt" => Ok("<"),
        "$lte" => Ok("<="),
        "$in" => Ok("IN"),
        "$contains" => Ok("CONTAINS"),
        "$any" => Ok("CONTAINSANY"),
        "$all" => Ok("CONTAINSALL"),
        _ => Err(format!("unknown operator: {op}")),
    }
}

fn validate_field_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("field name cannot be empty".into());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(format!("invalid field name: {name}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn clause(filters: Value) -> String {
        parse_filters(&filters).unwrap().where_clause
    }

    fn bindings(filters: Value) -> Vec<(String, Value)> {
        parse_filters(&filters).unwrap().bindings
    }

    #[test]
    fn exact_match_string() {
        let c = clause(json!({"category": "docs"}));
        assert_eq!(c, "custom_attributes.category = $f0");
    }

    #[test]
    fn exact_match_number() {
        let r = parse_filters(&json!({"version": 2})).unwrap();
        assert_eq!(r.where_clause, "custom_attributes.version = $f0");
        assert_eq!(r.bindings[0].1, json!(2));
    }

    #[test]
    fn exact_match_bool() {
        let c = clause(json!({"active": true}));
        assert_eq!(c, "custom_attributes.active = $f0");
    }

    #[test]
    fn null_is_none() {
        let c = clause(json!({"deleted": null}));
        assert_eq!(c, "custom_attributes.deleted IS NONE");
    }

    #[test]
    fn op_eq() {
        let c = clause(json!({"v": {"$eq": 1}}));
        assert_eq!(c, "custom_attributes.v = $f0");
    }

    #[test]
    fn op_ne() {
        let c = clause(json!({"v": {"$ne": 0}}));
        assert_eq!(c, "custom_attributes.v != $f0");
    }

    #[test]
    fn op_gt() {
        let c = clause(json!({"v": {"$gt": 1}}));
        assert_eq!(c, "custom_attributes.v > $f0");
    }

    #[test]
    fn op_gte() {
        let c = clause(json!({"v": {"$gte": 2}}));
        assert_eq!(c, "custom_attributes.v >= $f0");
    }

    #[test]
    fn op_lt() {
        let c = clause(json!({"v": {"$lt": 10}}));
        assert_eq!(c, "custom_attributes.v < $f0");
    }

    #[test]
    fn op_lte() {
        let c = clause(json!({"v": {"$lte": 5}}));
        assert_eq!(c, "custom_attributes.v <= $f0");
    }

    #[test]
    fn op_in() {
        let c = clause(json!({"status": {"$in": ["a", "b"]}}));
        assert_eq!(c, "custom_attributes.status IN $f0");
    }

    #[test]
    fn op_contains() {
        let c = clause(json!({"tags": {"$contains": "api"}}));
        assert_eq!(c, "custom_attributes.tags CONTAINS $f0");
    }

    #[test]
    fn op_any() {
        let c = clause(json!({"tags": {"$any": ["api", "gql"]}}));
        assert_eq!(c, "custom_attributes.tags CONTAINSANY $f0");
    }

    #[test]
    fn op_all() {
        let c = clause(json!({"tags": {"$all": ["api", "rest"]}}));
        assert_eq!(c, "custom_attributes.tags CONTAINSALL $f0");
    }

    #[test]
    fn implicit_and_multiple_fields() {
        let r = parse_filters(&json!({"a": 1, "b": 2})).unwrap();
        assert!(r.where_clause.contains("custom_attributes.a = $f"));
        assert!(r.where_clause.contains("custom_attributes.b = $f"));
        assert!(r.where_clause.contains(" AND "));
    }

    #[test]
    fn explicit_and() {
        let c = clause(json!({
            "$and": [
                {"a": 1},
                {"b": 2}
            ]
        }));
        assert_eq!(c, "(custom_attributes.a = $f0 AND custom_attributes.b = $f1)");
    }

    #[test]
    fn explicit_or() {
        let c = clause(json!({
            "$or": [
                {"category": "docs"},
                {"category": "logs"}
            ]
        }));
        assert_eq!(c, "(custom_attributes.category = $f0 OR custom_attributes.category = $f1)");
    }

    #[test]
    fn nested_and_or() {
        let c = clause(json!({
            "$or": [
                {"$and": [{"a": 1}, {"b": 2}]},
                {"c": 3}
            ]
        }));
        assert_eq!(
            c,
            "((custom_attributes.a = $f0 AND custom_attributes.b = $f1) OR custom_attributes.c = $f2)"
        );
    }

    #[test]
    fn nested_path() {
        let c = clause(json!({"metadata": {"nested": {"field": {"$gte": 2}}}}));
        assert_eq!(c, "custom_attributes.metadata.nested.field >= $f0");
    }

    #[test]
    fn nested_path_exact_match() {
        let c = clause(json!({"meta": {"env": "prod"}}));
        assert_eq!(c, "custom_attributes.meta.env = $f0");
    }

    #[test]
    fn mixed_operators_and_fields() {
        let r = parse_filters(&json!({
            "version": {"$gte": 2},
            "tags": {"$contains": "api"}
        }))
        .unwrap();
        assert!(r.where_clause.contains("custom_attributes.version >= $f"));
        assert!(r.where_clause.contains("custom_attributes.tags CONTAINS $f"));
        assert!(r.where_clause.contains(" AND "));
    }

    #[test]
    fn or_with_nested_path() {
        let c = clause(json!({
            "$or": [
                {"config": {"env": "prod"}},
                {"config": {"env": "staging"}}
            ]
        }));
        assert_eq!(
            c,
            "(custom_attributes.config.env = $f0 OR custom_attributes.config.env = $f1)"
        );
    }

    #[test]
    fn bindings_contain_correct_values() {
        let b = bindings(json!({"category": "docs", "version": 3}));
        let values: Vec<&Value> = b.iter().map(|(_, v)| v).collect();
        assert!(values.contains(&&json!("docs")));
        assert!(values.contains(&&json!(3)));
    }

    #[test]
    fn unknown_operator_is_error() {
        let r = parse_filters(&json!({"x": {"$like": "foo"}}));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("unknown operator"));
    }

    #[test]
    fn operator_without_path_is_error() {
        let r = parse_filters(&json!({"$eq": 1}));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("requires a field path"));
    }

    #[test]
    fn invalid_field_name_is_error() {
        let r = parse_filters(&json!({"field; DROP TABLE": "x"}));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("invalid field name"));
    }

    #[test]
    fn empty_filters_is_error() {
        let r = parse_filters(&json!({}));
        assert!(r.is_err());
    }

    #[test]
    fn non_object_filters_is_error() {
        let r = parse_filters(&json!("not an object"));
        assert!(r.is_err());
    }

    #[test]
    fn or_with_non_array_is_error() {
        let r = parse_filters(&json!({"$or": "bad"}));
        assert!(r.is_err());
    }

    #[test]
    fn and_with_non_object_elements_is_error() {
        let r = parse_filters(&json!({"$and": [1, 2]}));
        assert!(r.is_err());
    }
}
