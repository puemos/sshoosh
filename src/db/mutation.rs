#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum QueryMutation {
    Insert { table: String },
    Update { table: String },
    Delete { table: String },
    Replace { table: String },
    Create { target: String },
    Alter { target: String },
    Drop { target: String },
    Vacuum,
    Truncate { target: String },
    Attach,
    Detach,
    Pragma,
    ReadOnly,
}

pub(super) fn query_is_write(sql: &str) -> bool {
    !matches!(query_mutation(sql), Some(QueryMutation::ReadOnly))
}

pub(super) fn query_mutation(sql: &str) -> Option<QueryMutation> {
    let normalized = normalize_sql(sql);
    let statement = if normalized.starts_with("with ") {
        strip_with_clause_prefix(&normalized)
    } else {
        normalized.as_str()
    };
    let mut tokens = statement.split_whitespace();
    let token = tokens.next()?;
    match token {
        "insert" => parse_insert(tokens),
        "select" | "explain" => Some(QueryMutation::ReadOnly),
        "or" => match tokens.next()? {
            "replace" => parse_insert(tokens).map(|mutation| match mutation {
                QueryMutation::Insert { table } => QueryMutation::Replace { table },
                _ => mutation,
            }),
            "ignore" | "rollback" | "abort" | "fail" => parse_insert(tokens),
            "update" => parse_update(tokens),
            _ => None,
        },
        "update" => parse_update(tokens),
        "delete" => {
            let token = tokens.next()?;
            if token == "from" {
                tokens.next().map(|table| QueryMutation::Delete {
                    table: table.to_string(),
                })
            } else {
                None
            }
        }
        "replace" => parse_insert(tokens).map(|m| match m {
            QueryMutation::Insert { table } => QueryMutation::Replace { table },
            _ => m,
        }),
        "create" | "alter" | "drop" | "truncate" | "attach" | "detach" => {
            parse_schema_mutation(token, &mut tokens)
        }
        "vacuum" => Some(QueryMutation::Vacuum),
        "begin" | "commit" | "rollback" | "savepoint" | "release" => Some(QueryMutation::ReadOnly),
        "pragma" => {
            if statement.contains(" = ") {
                Some(QueryMutation::Pragma)
            } else {
                Some(QueryMutation::ReadOnly)
            }
        }
        _ => None,
    }
}

fn strip_with_clause_prefix(sql: &str) -> &str {
    let mut depth = 0usize;
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] as char {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                if depth > 0 {
                    depth = depth.saturating_sub(1);
                }
                if depth == 0 {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b',' {
                        i = j;
                        continue;
                    }
                    if j < bytes.len() {
                        return &sql[j..];
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    sql
}

fn parse_insert<'a, I>(mut tokens: I) -> Option<QueryMutation>
where
    I: Iterator<Item = &'a str>,
{
    let mut token = tokens.next()?;
    if token == "or" {
        token = tokens.next()?;
    }
    let table = if token == "into" {
        tokens.next()?
    } else {
        token
    };
    Some(QueryMutation::Insert {
        table: table.to_string(),
    })
}

fn parse_update<'a, I>(mut tokens: I) -> Option<QueryMutation>
where
    I: Iterator<Item = &'a str>,
{
    let table = tokens.next()?;
    Some(QueryMutation::Update {
        table: table.to_string(),
    })
}

fn parse_schema_mutation<'a>(
    token: &str,
    mut tokens: impl Iterator<Item = &'a str>,
) -> Option<QueryMutation> {
    let target = tokens
        .next()
        .map(|target| strip_schema_qualifier(token, target));
    target.map(|target| match token {
        "create" => QueryMutation::Create { target },
        "alter" => QueryMutation::Alter { target },
        "drop" => QueryMutation::Drop { target },
        "truncate" => QueryMutation::Truncate { target },
        "attach" => QueryMutation::Attach,
        "detach" => QueryMutation::Detach,
        _ => QueryMutation::Create { target },
    })
}

fn strip_schema_qualifier(token: &str, value: &str) -> String {
    if token == "attach" || token == "detach" {
        value.to_string()
    } else {
        value.trim_matches(&['"', '`', '\''][..]).to_string()
    }
}

pub(super) fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}
