#![forbid(unsafe_code)]
//! Database I/O Handler —— 基于 `sqlx` 接入 SQLite。
//!
//! 执行 SQL 语句并将结果转换为 `JsonValue`：
//! - `SELECT` 等查询语句返回 `JsonValue::Array`（每行是一个 `JsonValue::Object`）。
//! - `INSERT`/`UPDATE`/`DELETE` 等非查询语句返回受影响行数 `JsonValue::Integer`。
//!
//! 参数通过 `?` 占位符绑定，避免 SQL 注入。

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqliteQueryResult, SqliteRow};
use sqlx::{Column, Row};
use tier0_tcb::JsonValue;

use crate::io_handler::{IoHandler, IoResult};

/// 单次 DB 查询超时（P0-2：DB 5s）
const DB_TIMEOUT: Duration = Duration::from_secs(5);

/// SQLite 处理器
///
/// 持有 `sqlx::SqlitePool` 连接池，执行 SQL 查询。
pub struct DbHandler {
    pool: SqlitePool,
}

impl DbHandler {
    /// 异步初始化连接池。
    ///
    /// 等价于 [`DbHandler::connect`]，传入完整的数据库 URL（如 `sqlite::memory:`
    /// 或 `sqlite://path/to/db.sqlite`）。
    pub async fn new(database_url: String) -> Result<Self, sqlx::Error> {
        Self::connect(&database_url).await
    }

    /// 异步连接数据库并创建连接池。
    ///
    /// # 参数
    /// - `database_url`: SQLite 连接字符串。
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(database_url).await?;
        Ok(Self { pool })
    }

    /// 通过文件路径异步连接 SQLite，自动创建不存在的文件。
    ///
    /// 在 Windows 上避免 URL 反斜杠解析问题，推荐使用此方法。
    ///
    /// # 参数
    /// - `path`: 数据库文件路径（如 `./data/demo.sqlite`）。
    pub async fn connect_file(path: impl AsRef<Path>) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
        Ok(Self { pool })
    }
}

impl IoHandler for DbHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // 提取 SQL（必需）
        let query_str = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param: query".to_string())?;

        // 构建查询并绑定参数（可选，数组形式）
        let mut query = sqlx::query(query_str);
        if let Some(args) = params.get("params").and_then(|v| v.as_array()) {
            for arg in args {
                query = match arg {
                    JsonValue::Integer(i) => query.bind(*i),
                    JsonValue::String(s) => query.bind(s.clone()),
                    JsonValue::Bool(b) => query.bind(*b),
                    JsonValue::Null => query.bind(Option::<i64>::None),
                    // 复合类型序列化为 JSON 文本
                    JsonValue::Array(_) | JsonValue::Object(_) => query.bind(arg.to_string()),
                };
            }
        }

        // 根据 SQL 语句首词判断是查询还是非查询
        let is_query = query_str
            .trim_start()
            .to_ascii_uppercase()
            .starts_with("SELECT");

        if is_query {
            // P0-2：5s 超时，防止 DB 卡住导致会话僵死
            let rows: Vec<SqliteRow> =
                tokio::time::timeout(DB_TIMEOUT, query.fetch_all(&self.pool))
                    .await
                    .map_err(|_| format!("db query timed out after {}s", DB_TIMEOUT.as_secs()))?
                    .map_err(|e| e.to_string())?;
            let arr: Vec<JsonValue> = rows.iter().map(row_to_json).collect();
            Ok(JsonValue::Array(arr))
        } else {
            let result: SqliteQueryResult =
                tokio::time::timeout(DB_TIMEOUT, query.execute(&self.pool))
                    .await
                    .map_err(|_| format!("db execute timed out after {}s", DB_TIMEOUT.as_secs()))?
                    .map_err(|e| e.to_string())?;
            Ok(JsonValue::Integer(result.rows_affected() as i64))
        }
    }
}

/// 将一行 `SqliteRow` 转换为 `JsonValue::Object`。
///
/// 按列类型依次尝试 i64 / bool / String / f64 解码，
/// 无法解码的列回退为 `JsonValue::Null`。
fn row_to_json(row: &SqliteRow) -> JsonValue {
    let mut obj: BTreeMap<String, JsonValue> = BTreeMap::new();
    for (idx, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let value = if let Ok(Some(i)) = row.try_get::<Option<i64>, _>(idx) {
            JsonValue::Integer(i)
        } else if let Ok(Some(b)) = row.try_get::<Option<bool>, _>(idx) {
            JsonValue::Bool(b)
        } else if let Ok(Some(s)) = row.try_get::<Option<String>, _>(idx) {
            JsonValue::String(s)
        } else if let Ok(Some(f)) = row.try_get::<Option<f64>, _>(idx) {
            // JsonValue 无浮点类型，用字符串保留精度
            JsonValue::String(f.to_string())
        } else {
            JsonValue::Null
        };
        obj.insert(name, value);
    }
    JsonValue::Object(obj)
}
