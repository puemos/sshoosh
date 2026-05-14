use super::encryption::{ENVELOPE_PREFIX, EncryptionService, encryption_tables_for_column};
use super::fs::secure_local_database_files;
use super::mutation::{normalize_sql, query_is_write};
use super::query_encryption::encrypt_query_params;
use super::*;

pub struct DbTransaction {
    tx: libsql::Transaction,
    encryption: Option<Arc<EncryptionService>>,
    bypass_master_check: bool,
    is_master: Arc<AtomicBool>,
}

pub struct DbReadSession {
    conn: Connection,
    encryption: Option<Arc<EncryptionService>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbResult {
    rows_affected: u64,
    last_insert_rowid: i64,
}

impl DbResult {
    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.last_insert_rowid
    }
}

#[derive(Clone)]
pub struct DbRow {
    values: Vec<Value>,
    names: Vec<String>,
    columns: HashMap<String, usize>,
    row_id_hint: Option<String>,
    encryption: Option<Arc<EncryptionService>>,
}

pub trait FromDbValue: Sized {
    fn from_db_value(value: Value) -> anyhow::Result<Self>;
}

pub trait IntoDbValue {
    fn into_db_value(self) -> Value;
}

#[derive(Clone, Debug)]
pub struct Query {
    sql: String,
    params: Vec<Value>,
    bypass_master_check: bool,
}

pub struct QueryScalar<T> {
    inner: Query,
    _marker: PhantomData<T>,
}

pub struct QueryAs<T> {
    inner: Query,
    _marker: PhantomData<T>,
}

pub trait FromDbRow: Sized {
    fn from_db_row(row: DbRow) -> anyhow::Result<Self>;
}

pub fn query(sql: &str) -> Query {
    Query {
        sql: sql.to_string(),
        params: Vec::new(),
        bypass_master_check: false,
    }
}

pub fn query_scalar<T>(sql: &str) -> QueryScalar<T> {
    QueryScalar {
        inner: query(sql),
        _marker: PhantomData,
    }
}

pub fn query_as<T>(sql: &str) -> QueryAs<T> {
    QueryAs {
        inner: query(sql),
        _marker: PhantomData,
    }
}

impl Database {
    pub async fn transaction(&self) -> anyhow::Result<DbTransaction> {
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        Ok(DbTransaction {
            tx,
            encryption: self.encryption.clone(),
            bypass_master_check: false,
            is_master: self.is_master.clone(),
        })
    }

    pub async fn transaction_unchecked(&self) -> anyhow::Result<DbTransaction> {
        let mut tx = self.transaction().await?;
        tx.bypass_master_check = true;
        Ok(tx)
    }

    pub async fn begin(&self) -> anyhow::Result<DbTransaction> {
        self.transaction().await
    }

    pub async fn read_session(&self) -> anyhow::Result<DbReadSession> {
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        Ok(DbReadSession {
            conn,
            encryption: self.encryption.clone(),
        })
    }

    pub(super) fn connection(&self) -> anyhow::Result<Connection> {
        self.inner.connect().map_err(Into::into)
    }

    pub(super) async fn configure_connection(&self, conn: &Connection) -> anyhow::Result<()> {
        if self.kind == DatabaseKind::Local {
            let _ = conn.busy_timeout(Duration::from_secs(5));
            conn.execute("PRAGMA foreign_keys = ON", ()).await?;
            conn.execute("PRAGMA temp_store = MEMORY", ()).await?;
            conn.execute("PRAGMA journal_mode = WAL", ()).await.ok();
            conn.execute("PRAGMA synchronous = NORMAL", ()).await.ok();
            if let Some(path) = self.local_path.as_deref() {
                secure_local_database_files(path)?;
            }
            if self.ignore_check_constraints.load(Ordering::Acquire) {
                conn.execute("PRAGMA ignore_check_constraints = ON", ())
                    .await
                    .ok();
            }
        }
        Ok(())
    }

    pub(super) async fn execute_batch_unchecked(&self, sql: &str) -> anyhow::Result<()> {
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        for statement in sql
            .split(';')
            .map(str::trim)
            .filter(|stmt| !stmt.is_empty())
        {
            conn.execute(statement, ()).await.with_context(|| {
                format!("executing migration SQL: {}", summarize_sql(statement))
            })?;
        }
        Ok(())
    }
}

impl DbTransaction {
    pub async fn commit(self) -> anyhow::Result<()> {
        self.tx.commit().await?;
        Ok(())
    }

    pub async fn rollback(self) -> anyhow::Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }
}

#[allow(async_fn_in_trait)]
pub trait DbExecutor {
    async fn execute_query(&mut self, query: Query) -> anyhow::Result<DbResult>;
    async fn fetch_rows(&mut self, query: Query) -> anyhow::Result<Vec<DbRow>>;
}

fn require_master_for_write(
    role: DbRole,
    allow_standby_writes: bool,
    query: &Query,
) -> anyhow::Result<()> {
    if query.bypass_master_check {
        return Ok(());
    }
    if !allow_standby_writes && !role.allow_standby_writes() && query.is_write() {
        bail!("master lease required for write query");
    }
    Ok(())
}

impl DbExecutor for &Database {
    async fn execute_query(&mut self, mut query: Query) -> anyhow::Result<DbResult> {
        if normalize_sql(&query.sql).starts_with("pragma ignore_check_constraints = on") {
            self.ignore_check_constraints.store(true, Ordering::Release);
        }
        require_master_for_write(self.role(), false, &query)?;
        query.encrypt_params(self.encryption.as_deref())?;
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        let started = Instant::now();
        let rows_affected = conn
            .execute(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("executing SQL: {}", summarize_sql(&query.sql)))?;
        trace_query(
            "execute",
            &query.sql,
            started.elapsed(),
            Some(rows_affected),
            None,
        );
        let mut rows = conn.query("SELECT last_insert_rowid()", ()).await?;
        let last_insert_rowid = rows
            .next()
            .await?
            .and_then(|row| row.get::<i64>(0).ok())
            .unwrap_or(0);
        Ok(DbResult {
            rows_affected,
            last_insert_rowid,
        })
    }

    async fn fetch_rows(&mut self, query: Query) -> anyhow::Result<Vec<DbRow>> {
        require_master_for_write(self.role(), false, &query)?;
        let conn = self.connection()?;
        self.configure_connection(&conn).await?;
        let row_id_hint = query.row_id_hint();
        let sql = query.sql.clone();
        let started = Instant::now();
        let mut rows = conn
            .query(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("querying SQL: {}", summarize_sql(&query.sql)))?;
        let rows = collect_rows(&mut rows, self.encryption.clone(), row_id_hint).await?;
        trace_query("query", &sql, started.elapsed(), None, Some(rows.len()));
        Ok(rows)
    }
}

impl DbExecutor for &DbReadSession {
    async fn execute_query(&mut self, mut query: Query) -> anyhow::Result<DbResult> {
        require_master_for_write(DbRole::Standby, false, &query)?;
        query.encrypt_params(self.encryption.as_deref())?;
        let started = Instant::now();
        let rows_affected = self
            .conn
            .execute(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("executing SQL: {}", summarize_sql(&query.sql)))?;
        trace_query(
            "execute",
            &query.sql,
            started.elapsed(),
            Some(rows_affected),
            None,
        );
        let mut rows = self.conn.query("SELECT last_insert_rowid()", ()).await?;
        let last_insert_rowid = rows
            .next()
            .await?
            .and_then(|row| row.get::<i64>(0).ok())
            .unwrap_or(0);
        Ok(DbResult {
            rows_affected,
            last_insert_rowid,
        })
    }

    async fn fetch_rows(&mut self, query: Query) -> anyhow::Result<Vec<DbRow>> {
        require_master_for_write(DbRole::Standby, false, &query)?;
        let row_id_hint = query.row_id_hint();
        let sql = query.sql.clone();
        let started = Instant::now();
        let mut rows = self
            .conn
            .query(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("querying SQL: {}", summarize_sql(&query.sql)))?;
        let rows = collect_rows(&mut rows, self.encryption.clone(), row_id_hint).await?;
        trace_query("query", &sql, started.elapsed(), None, Some(rows.len()));
        Ok(rows)
    }
}

impl DbExecutor for &mut DbTransaction {
    async fn execute_query(&mut self, mut query: Query) -> anyhow::Result<DbResult> {
        if self.bypass_master_check {
            query.bypass_master_check = true;
        }
        require_master_for_write(self_role(self), self.bypass_master_check, &query)?;
        query.encrypt_params(self.encryption.as_deref())?;
        let started = Instant::now();
        let rows_affected = self
            .tx
            .execute(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("executing SQL: {}", summarize_sql(&query.sql)))?;
        trace_query(
            "execute",
            &query.sql,
            started.elapsed(),
            Some(rows_affected),
            None,
        );
        let mut rows = self.tx.query("SELECT last_insert_rowid()", ()).await?;
        let last_insert_rowid = rows
            .next()
            .await?
            .and_then(|row| row.get::<i64>(0).ok())
            .unwrap_or(0);
        Ok(DbResult {
            rows_affected,
            last_insert_rowid,
        })
    }

    async fn fetch_rows(&mut self, mut query: Query) -> anyhow::Result<Vec<DbRow>> {
        if self.bypass_master_check {
            query.bypass_master_check = true;
        }
        require_master_for_write(self_role(self), self.bypass_master_check, &query)?;
        let row_id_hint = query.row_id_hint();
        let sql = query.sql.clone();
        let started = Instant::now();
        let mut rows = self
            .tx
            .query(&query.sql, params_from_iter(query.params))
            .await
            .with_context(|| format!("querying SQL: {}", summarize_sql(&query.sql)))?;
        let rows = collect_rows(&mut rows, self.encryption.clone(), row_id_hint).await?;
        trace_query("query", &sql, started.elapsed(), None, Some(rows.len()));
        Ok(rows)
    }
}

fn self_role(exec: &mut DbTransaction) -> DbRole {
    if exec.is_master.load(Ordering::Acquire) {
        DbRole::Master
    } else {
        DbRole::Standby
    }
}

impl DbExecutor for &mut &mut DbTransaction {
    async fn execute_query(&mut self, query: Query) -> anyhow::Result<DbResult> {
        DbExecutor::execute_query(&mut **self, query).await
    }

    async fn fetch_rows(&mut self, query: Query) -> anyhow::Result<Vec<DbRow>> {
        DbExecutor::fetch_rows(&mut **self, query).await
    }
}

impl Query {
    fn is_write(&self) -> bool {
        query_is_write(&self.sql)
    }

    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.params.push(value.into_db_value());
        self
    }

    pub fn unchecked(mut self) -> Self {
        self.bypass_master_check = true;
        self
    }

    pub async fn execute<E: DbExecutor>(self, mut exec: E) -> anyhow::Result<DbResult> {
        exec.execute_query(self).await
    }

    pub async fn execute_unchecked<E: DbExecutor>(self, mut exec: E) -> anyhow::Result<DbResult> {
        exec.execute_query(self.unchecked()).await
    }

    pub async fn fetch_all<E: DbExecutor>(self, mut exec: E) -> anyhow::Result<Vec<DbRow>> {
        exec.fetch_rows(self).await
    }

    pub async fn fetch_all_unchecked<E: DbExecutor>(
        self,
        mut exec: E,
    ) -> anyhow::Result<Vec<DbRow>> {
        exec.fetch_rows(self.unchecked()).await
    }

    pub async fn fetch_optional<E: DbExecutor>(self, mut exec: E) -> anyhow::Result<Option<DbRow>> {
        let mut rows = exec.fetch_rows(self).await?;
        Ok(rows.pop())
    }

    pub async fn fetch_optional_unchecked<E: DbExecutor>(
        self,
        mut exec: E,
    ) -> anyhow::Result<Option<DbRow>> {
        let mut rows = exec.fetch_rows(self.unchecked()).await?;
        Ok(rows.pop())
    }

    pub async fn fetch_one<E: DbExecutor>(self, exec: E) -> anyhow::Result<DbRow> {
        self.fetch_optional(exec)
            .await?
            .context("query returned no rows")
    }

    pub async fn fetch_one_unchecked<E: DbExecutor>(self, exec: E) -> anyhow::Result<DbRow> {
        self.fetch_optional_unchecked(exec)
            .await?
            .context("query returned no rows")
    }

    fn encrypt_params(&mut self, encryption: Option<&EncryptionService>) -> anyhow::Result<()> {
        encrypt_query_params(encryption, &self.sql, &mut self.params)
    }

    fn row_id_hint(&self) -> Option<String> {
        let sql = normalize_sql(&self.sql);
        if sql.contains(" where id = ?") {
            self.params.first().and_then(value_as_str)
        } else {
            None
        }
    }
}

impl<T> QueryScalar<T> {
    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.inner = self.inner.bind(value);
        self
    }

    pub async fn fetch_one<E: DbExecutor>(self, exec: E) -> anyhow::Result<T>
    where
        T: FromDbValue,
    {
        let row = self.inner.fetch_one(exec).await?;
        row.try_get_idx(0)
    }

    pub async fn fetch_one_unchecked<E: DbExecutor>(self, exec: E) -> anyhow::Result<T>
    where
        T: FromDbValue,
    {
        let row = self.inner.fetch_one_unchecked(exec).await?;
        row.try_get_idx(0)
    }

    pub async fn fetch_optional<E: DbExecutor>(self, exec: E) -> anyhow::Result<Option<T>>
    where
        T: FromDbValue,
    {
        self.inner
            .fetch_optional(exec)
            .await?
            .map(|row| row.try_get_idx(0))
            .transpose()
    }

    pub async fn fetch_optional_unchecked<E: DbExecutor>(self, exec: E) -> anyhow::Result<Option<T>>
    where
        T: FromDbValue,
    {
        self.inner
            .fetch_optional_unchecked(exec)
            .await?
            .map(|row| row.try_get_idx(0))
            .transpose()
    }

    pub async fn fetch_all<E: DbExecutor>(self, exec: E) -> anyhow::Result<Vec<T>>
    where
        T: FromDbValue,
    {
        self.inner
            .fetch_all(exec)
            .await?
            .into_iter()
            .map(|row| row.try_get_idx(0))
            .collect()
    }
}

impl<T> QueryAs<T> {
    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.inner = self.inner.bind(value);
        self
    }

    pub async fn fetch_optional<E: DbExecutor>(self, exec: E) -> anyhow::Result<Option<T>>
    where
        T: FromDbRow,
    {
        self.inner
            .fetch_optional(exec)
            .await?
            .map(T::from_db_row)
            .transpose()
    }
}

impl DbRow {
    pub fn get<T: FromDbValue>(&self, name: &str) -> anyhow::Result<T> {
        self.try_get(name)
    }

    pub fn try_get<T: FromDbValue>(&self, name: &str) -> anyhow::Result<T> {
        let idx = self
            .columns
            .get(name)
            .copied()
            .or_else(|| self.columns.get(&name.to_ascii_lowercase()).copied())
            .with_context(|| format!("column not found: {name}"))?;
        self.try_get_idx(idx)
    }

    pub fn get_idx<T: FromDbValue>(&self, idx: usize) -> anyhow::Result<T> {
        self.try_get_idx(idx)
    }

    pub fn try_get_idx<T: FromDbValue>(&self, idx: usize) -> anyhow::Result<T> {
        let value = self
            .values
            .get(idx)
            .cloned()
            .with_context(|| format!("column index out of range: {idx}"))?;
        T::from_db_value(self.decrypt_value(idx, value)?)
    }

    pub fn columns(&self) -> Vec<String> {
        self.names.clone()
    }

    fn decrypt_value(&self, idx: usize, value: Value) -> anyhow::Result<Value> {
        let Value::Text(text) = value else {
            return Ok(value);
        };
        if !text.starts_with(ENVELOPE_PREFIX) {
            return Ok(Value::Text(text));
        }
        let Some(encryption) = self.encryption.as_deref() else {
            bail!("encrypted content exists but SSHOOSH_ENCRYPTION_KEY is not configured");
        };
        let id = self
            .columns
            .get("id")
            .and_then(|id_idx| self.values.get(*id_idx))
            .and_then(value_as_str)
            .or_else(|| self.row_id_hint.clone())
            .unwrap_or_default();
        let column = self
            .columns
            .iter()
            .find_map(|(name, col_idx)| {
                (*col_idx == idx && !name.is_empty()).then_some(name.as_str())
            })
            .unwrap_or("");
        for table in encryption_tables_for_column(column) {
            if let Ok(plain) = encryption.decrypt(table, &id, column, &text) {
                return Ok(Value::Text(plain));
            }
        }
        bail!("failed to decrypt encrypted database content; check SSHOOSH_ENCRYPTION_KEY")
    }
}

pub(super) fn value_as_str(value: &Value) -> Option<String> {
    match value {
        Value::Text(value) => Some(value.clone()),
        Value::Integer(value) => Some(value.to_string()),
        _ => None,
    }
}

impl FromDbValue for String {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Text(value) => Ok(value),
            Value::Integer(value) => Ok(value.to_string()),
            Value::Real(value) => Ok(value.to_string()),
            Value::Null => bail!("unexpected NULL string"),
            Value::Blob(_) => bail!("unexpected BLOB string"),
        }
    }
}

impl FromDbValue for Option<String> {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Null => Ok(None),
            value => String::from_db_value(value).map(Some),
        }
    }
}

impl FromDbValue for i64 {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Integer(value) => Ok(value),
            Value::Text(value) => Ok(value.parse()?),
            Value::Null => bail!("unexpected NULL integer"),
            _ => bail!("unexpected non-integer value"),
        }
    }
}

impl FromDbValue for Option<i64> {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::Null => Ok(None),
            value => i64::from_db_value(value).map(Some),
        }
    }
}

impl FromDbValue for bool {
    fn from_db_value(value: Value) -> anyhow::Result<Self> {
        Ok(i64::from_db_value(value)? != 0)
    }
}

impl FromDbRow for (Option<String>, Option<String>) {
    fn from_db_row(row: DbRow) -> anyhow::Result<Self> {
        Ok((row.try_get_idx(0)?, row.try_get_idx(1)?))
    }
}

impl IntoDbValue for Value {
    fn into_db_value(self) -> Value {
        self
    }
}

impl IntoDbValue for &str {
    fn into_db_value(self) -> Value {
        Value::Text(self.to_string())
    }
}

impl IntoDbValue for String {
    fn into_db_value(self) -> Value {
        Value::Text(self)
    }
}

impl IntoDbValue for &String {
    fn into_db_value(self) -> Value {
        Value::Text(self.clone())
    }
}

impl IntoDbValue for Option<&str> {
    fn into_db_value(self) -> Value {
        self.map(|value| Value::Text(value.to_string()))
            .unwrap_or(Value::Null)
    }
}

impl IntoDbValue for Option<String> {
    fn into_db_value(self) -> Value {
        self.map(Value::Text).unwrap_or(Value::Null)
    }
}

impl IntoDbValue for Option<i64> {
    fn into_db_value(self) -> Value {
        self.map(Value::Integer).unwrap_or(Value::Null)
    }
}

impl IntoDbValue for Option<&String> {
    fn into_db_value(self) -> Value {
        self.map(|value| Value::Text(value.clone()))
            .unwrap_or(Value::Null)
    }
}

impl IntoDbValue for i64 {
    fn into_db_value(self) -> Value {
        Value::Integer(self)
    }
}

impl IntoDbValue for i32 {
    fn into_db_value(self) -> Value {
        Value::Integer(self as i64)
    }
}

impl IntoDbValue for u64 {
    fn into_db_value(self) -> Value {
        Value::Integer(self as i64)
    }
}

impl IntoDbValue for bool {
    fn into_db_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

async fn collect_rows(
    rows: &mut libsql::Rows,
    encryption: Option<Arc<EncryptionService>>,
    row_id_hint: Option<String>,
) -> anyhow::Result<Vec<DbRow>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        let mut values = Vec::new();
        let mut names = Vec::new();
        let mut columns = HashMap::new();
        for idx in 0..row.column_count() {
            let name = row.column_name(idx).unwrap_or("").to_string();
            columns.insert(name.clone(), idx as usize);
            columns.insert(name.to_ascii_lowercase(), idx as usize);
            names.push(name);
            values.push(row.get_value(idx)?);
        }
        out.push(DbRow {
            values,
            names,
            columns,
            row_id_hint: row_id_hint.clone(),
            encryption: encryption.clone(),
        });
    }
    Ok(out)
}

fn summarize_sql(sql: &str) -> String {
    normalize_sql(sql).chars().take(160).collect()
}

fn trace_query(
    operation: &'static str,
    sql: &str,
    elapsed: Duration,
    rows_affected: Option<u64>,
    row_count: Option<usize>,
) {
    static SLOW_QUERY_MS: OnceLock<u128> = OnceLock::new();
    let slow_ms = *SLOW_QUERY_MS.get_or_init(|| {
        std::env::var("SSHOOSH_SLOW_QUERY_MS")
            .ok()
            .and_then(|value| value.parse::<u128>().ok())
            .unwrap_or(50)
    });
    if elapsed.as_millis() >= slow_ms {
        tracing::warn!(
            operation,
            elapsed_ms = elapsed.as_millis() as u64,
            rows_affected,
            row_count,
            sql = %summarize_sql(sql),
            "slow database query"
        );
    } else {
        tracing::trace!(
            operation,
            elapsed_ms = elapsed.as_millis() as u64,
            rows_affected,
            row_count,
            sql = %summarize_sql(sql),
            "database query"
        );
    }
}
