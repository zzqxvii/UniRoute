//! UniRoute 数据存储模块
//!
//! 简化架构：Provider 是核心实体，包含 baseUrl + API Key

use crate::models::{
    ApiFormat, AuthType, Group, GroupModel, ModelMapping, OAuthConfig, OAuthTokens, Provider,
    RequestLog,
};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// 请求统计
#[derive(Debug, Clone, serde::Serialize)]
pub struct RequestStats {
    pub total_requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
    pub avg_latency_ms: f64,
}

/// 数据库管理器
pub struct Database {
    conn: Mutex<Connection>,
    db_path: PathBuf,
}

impl Database {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        let db = Self {
            conn: Mutex::new(conn),
            db_path,
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".uniroute")
            .join("uniroute.db")
    }

    fn init_schema(&self) -> Result<()> {
        let db = self.conn.lock().unwrap();

        db.execute_batch(
            r#"
            -- Providers (供应商 = baseUrl + API Key)
            CREATE TABLE IF NOT EXISTS providers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                prefix TEXT NOT NULL UNIQUE,
                base_url TEXT NOT NULL,
                api_key TEXT,
                api_format TEXT NOT NULL DEFAULT 'openai',
                models TEXT,
                enable_cost INTEGER NOT NULL DEFAULT 0,
                auth_type TEXT NOT NULL DEFAULT 'api_key',
                oauth_config TEXT,
                oauth_tokens TEXT,
                headers TEXT,
                auth_header TEXT NOT NULL DEFAULT 'Authorization',
                auth_prefix TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                is_builtin INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            -- Groups (路由组)
            CREATE TABLE IF NOT EXISTS groups (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                strategy TEXT NOT NULL DEFAULT 'priority',
                config TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            -- Group Models
            CREATE TABLE IF NOT EXISTS group_models (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                group_id TEXT NOT NULL,
                model TEXT NOT NULL,
                weight INTEGER NOT NULL DEFAULT 1,
                priority INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (group_id) REFERENCES groups(id) ON DELETE CASCADE
            );

            -- Model Mappings (模型名 → Group)
            CREATE TABLE IF NOT EXISTS model_mappings (
                id TEXT PRIMARY KEY,
                pattern TEXT NOT NULL,
                group_id TEXT NOT NULL,
                priority INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (group_id) REFERENCES groups(id) ON DELETE CASCADE
            );

            -- Request Logs (请求日志)
            CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                requested_model TEXT,
                model TEXT,
                provider TEXT,
                provider_prefix TEXT,
                url TEXT,
                protocol_transform TEXT,
                status_code INTEGER,
                latency_ms INTEGER,
                first_token_ms INTEGER,
                prompt_tokens INTEGER,
                completion_tokens INTEGER,
                cost REAL,
                error TEXT,
                original_request_body TEXT,
                request_body TEXT,
                original_response_body TEXT,
                response_body TEXT
            );

            -- Settings
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Quota Usage (按天记录成本)
            CREATE TABLE IF NOT EXISTS quota_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL UNIQUE,
                total_cost REAL NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_providers_prefix ON providers(prefix);
            CREATE INDEX IF NOT EXISTS idx_groups_name ON groups(name);
            CREATE INDEX IF NOT EXISTS idx_group_models_group ON group_models(group_id);
            CREATE INDEX IF NOT EXISTS idx_request_logs_timestamp ON request_logs(timestamp);
            CREATE INDEX IF NOT EXISTS idx_request_logs_model ON request_logs(model);
            "#,
        )?;

        // 迁移：检查并添加缺失的列
        let columns: Vec<String> = db
            .prepare("PRAGMA table_info(request_logs)")?
            .query_map([], |row| row.get(1))?
            .collect::<Result<Vec<_>, _>>()?;

        if !columns.contains(&"provider_prefix".to_string()) {
            tracing::info!("添加 provider_prefix 列到 request_logs 表");
            db.execute(
                "ALTER TABLE request_logs ADD COLUMN provider_prefix TEXT",
                [],
            )?;
        }

        if !columns.contains(&"url".to_string()) {
            tracing::info!("添加 url 列到 request_logs 表");
            db.execute("ALTER TABLE request_logs ADD COLUMN url TEXT", [])?;
        }

        if !columns.contains(&"first_token_ms".to_string()) {
            tracing::info!("添加 first_token_ms 列到 request_logs 表");
            db.execute(
                "ALTER TABLE request_logs ADD COLUMN first_token_ms INTEGER",
                [],
            )?;
        }

        if !columns.contains(&"cost".to_string()) {
            tracing::info!("添加 cost 列到 request_logs 表");
            db.execute("ALTER TABLE request_logs ADD COLUMN cost REAL", [])?;
        }

        if !columns.contains(&"protocol_transform".to_string()) {
            tracing::info!("添加 protocol_transform 列到 request_logs 表");
            db.execute(
                "ALTER TABLE request_logs ADD COLUMN protocol_transform TEXT",
                [],
            )?;
        }

        if !columns.contains(&"original_response_body".to_string()) {
            tracing::info!("添加 original_response_body 列到 request_logs 表");
            db.execute(
                "ALTER TABLE request_logs ADD COLUMN original_response_body TEXT",
                [],
            )?;
        }

        if !columns.contains(&"original_request_body".to_string()) {
            tracing::info!("添加 original_request_body 列到 request_logs 表");
            db.execute(
                "ALTER TABLE request_logs ADD COLUMN original_request_body TEXT",
                [],
            )?;
        }

        if !columns.contains(&"original_input_tokens".to_string()) {
            tracing::info!("添加 original_input_tokens 列到 request_logs 表");
            db.execute(
                "ALTER TABLE request_logs ADD COLUMN original_input_tokens INTEGER",
                [],
            )?;
        }

        if !columns.contains(&"translated_input_tokens".to_string()) {
            tracing::info!("添加 translated_input_tokens 列到 request_logs 表");
            db.execute(
                "ALTER TABLE request_logs ADD COLUMN translated_input_tokens INTEGER",
                [],
            )?;
        }

        if !columns.contains(&"endpoint_type".to_string()) {
            tracing::info!("添加 endpoint_type 列到 request_logs 表");
            db.execute(
                "ALTER TABLE request_logs ADD COLUMN endpoint_type TEXT",
                [],
            )?;
        }

        // 添加 currency 列到 providers 表
        let provider_columns: Vec<String> = db
            .prepare("PRAGMA table_info(providers)")?
            .query_map([], |row| row.get(1))?
            .collect::<Result<Vec<_>, _>>()?;

        if !provider_columns.contains(&"currency".to_string()) {
            tracing::info!("添加 currency 列到 providers 表");
            db.execute(
                "ALTER TABLE providers ADD COLUMN currency TEXT NOT NULL DEFAULT 'CNY'",
                [],
            )?;
        }

        // 检查 providers 表是否存在
        let providers_columns: Vec<String> = db
            .prepare("PRAGMA table_info(providers)")?
            .query_map([], |row| row.get(1))?
            .collect::<Result<Vec<_>, _>>()?;

        // 添加 OAuth 相关列到 providers 表
        if !providers_columns.is_empty() {
            if !providers_columns.contains(&"enable_cost".to_string()) {
                tracing::info!("添加 enable_cost 列到 providers 表");
                db.execute(
                    "ALTER TABLE providers ADD COLUMN enable_cost INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
            }
            if !providers_columns.contains(&"auth_type".to_string()) {
                tracing::info!("添加 auth_type 列到 providers 表");
                db.execute(
                    "ALTER TABLE providers ADD COLUMN auth_type TEXT NOT NULL DEFAULT 'api_key'",
                    [],
                )?;
            }
            if !providers_columns.contains(&"oauth_config".to_string()) {
                tracing::info!("添加 oauth_config 列到 providers 表");
                db.execute("ALTER TABLE providers ADD COLUMN oauth_config TEXT", [])?;
            }
            if !providers_columns.contains(&"oauth_tokens".to_string()) {
                tracing::info!("添加 oauth_tokens 列到 providers 表");
                db.execute("ALTER TABLE providers ADD COLUMN oauth_tokens TEXT", [])?;
            }
            if !providers_columns.contains(&"headers".to_string()) {
                tracing::info!("添加 headers 列到 providers 表");
                db.execute("ALTER TABLE providers ADD COLUMN headers TEXT", [])?;
            }
            if !providers_columns.contains(&"auth_header".to_string()) {
                tracing::info!("添加 auth_header 列到 providers 表");
                db.execute("ALTER TABLE providers ADD COLUMN auth_header TEXT NOT NULL DEFAULT 'Authorization'", [])?;
            }
            if !providers_columns.contains(&"auth_prefix".to_string()) {
                tracing::info!("添加 auth_prefix 列到 providers 表");
                db.execute("ALTER TABLE providers ADD COLUMN auth_prefix TEXT", [])?;
            }
        }

        // 检查 group_models 表是否存在 enabled 列
        let group_models_columns: Vec<String> = db
            .prepare("PRAGMA table_info(group_models)")?
            .query_map([], |row| row.get(1))?
            .collect::<Result<Vec<_>, _>>()?;

        if !group_models_columns.contains(&"enabled".to_string()) {
            tracing::info!("添加 enabled 列到 group_models 表");
            db.execute(
                "ALTER TABLE group_models ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
                [],
            )?;
        }

        // 添加 endpoint_type 列到 groups 表
        let groups_columns: Vec<String> = db
            .prepare("SELECT name FROM pragma_table_info('groups')")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        if !groups_columns.contains(&"endpoint_type".to_string()) {
            tracing::info!("添加 endpoint_type 列到 groups 表");
            db.execute(
                "ALTER TABLE groups ADD COLUMN endpoint_type TEXT",
                [],
            )?;
        }

        // 重建 groups 表，移除 name UNIQUE 约束，改为 (name, endpoint_type) 复合唯一
        // 检查是否还有 name 的 UNIQUE 约束
        let has_name_unique: bool = db
            .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='groups'")?
            .query_row([], |row| {
                let sql: String = row.get(0)?;
                Ok(sql.contains("name TEXT NOT NULL UNIQUE"))
            })?;

        if has_name_unique {
            tracing::info!("重建 groups 表，修改唯一约束为 (name, endpoint_type)");
            db.execute_batch(&[
                // 1. 创建新表
                "CREATE TABLE groups_new (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    description TEXT,
                    strategy TEXT NOT NULL DEFAULT 'priority',
                    config TEXT,
                    is_active INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    endpoint_type TEXT
                )",
                // 2. 复制数据
                "INSERT INTO groups_new SELECT id, name, description, strategy, config, is_active, created_at, updated_at, endpoint_type FROM groups",
                // 3. 删除旧表
                "DROP TABLE groups",
                // 4. 重命名新表
                "ALTER TABLE groups_new RENAME TO groups",
                // 5. 创建复合唯一索引
                "CREATE UNIQUE INDEX idx_groups_name_endpoint ON groups(name, COALESCE(endpoint_type, 'chat'))",
            ].join("; "))?;
        }

        Ok(())
    }

    // ============ Providers ============

    pub fn save_provider(&self, provider: &Provider) -> Result<()> {
        let db = self.conn.lock().unwrap();
        db.execute(
            r#"INSERT INTO providers (id, name, prefix, base_url, api_key, api_format, models, enable_cost, currency, auth_type, oauth_config, oauth_tokens, headers, auth_header, auth_prefix, is_active, is_builtin, created_at, updated_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
               ON CONFLICT(id) DO UPDATE SET
                 name=?2, prefix=?3, base_url=?4, api_key=?5, api_format=?6, models=?7, enable_cost=?8, currency=?9, auth_type=?10, oauth_config=?11, oauth_tokens=?12, headers=?13, auth_header=?14, auth_prefix=?15, is_active=?16, updated_at=?19"#,
            rusqlite::params![
                provider.id,
                provider.name,
                provider.prefix,
                provider.base_url,
                provider.api_key,
                serde_json::to_string(&provider.api_format)?,
                serde_json::to_string(&provider.models)?,
                provider.enable_cost as i32,
                &provider.currency,
                serde_json::to_string(&provider.auth_type)?,
                provider.oauth.as_ref().map(|o| serde_json::to_string(o).ok()).flatten(),
                provider.oauth_tokens.as_ref().map(|t| serde_json::to_string(t).ok()).flatten(),
                serde_json::to_string(&provider.headers)?,
                provider.auth_header,
                provider.auth_prefix,
                provider.is_active as i32,
                provider.is_builtin as i32,
                provider.created_at.to_rfc3339(),
                provider.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_providers(&self) -> Result<Vec<Provider>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name, prefix, base_url, api_key, api_format, models, enable_cost, currency, auth_type, oauth_config, oauth_tokens, headers, auth_header, auth_prefix, is_active, is_builtin, created_at, updated_at
             FROM providers ORDER BY name"
        )?;
        let providers = stmt
            .query_map([], |row| {
                let api_format_str: String = row.get(5)?;
                let models_str: String = row.get(6)?;
                let auth_type_str: Option<String> = row.get(9)?;
                let oauth_config_str: Option<String> = row.get(10)?;
                let oauth_tokens_str: Option<String> = row.get(11)?;
                let headers_str: Option<String> = row.get(12)?;
                let auth_header: Option<String> = row.get(13)?;
                let auth_prefix: Option<String> = row.get(14)?;
                let currency: String = row.get(8).unwrap_or_else(|_| "CNY".to_string());
                let created_str: String = row.get(17)?;
                let updated_str: String = row.get(18)?;

                // 尝试解析新格式的 models (ModelConfig)，如果失败则尝试旧格式 (String)
                let models: Vec<crate::models::ModelConfig> =
                    if models_str.starts_with('[') && models_str.contains("\"name\"") {
                        serde_json::from_str(&models_str).unwrap_or_default()
                    } else {
                        // 旧格式：字符串数组，转换为 ModelConfig
                        let old_models: Vec<String> =
                            serde_json::from_str(&models_str).unwrap_or_default();
                        old_models
                            .into_iter()
                            .map(|name| crate::models::ModelConfig::from(name))
                            .collect()
                    };

                // 解析 auth_type
                let auth_type: AuthType = auth_type_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(AuthType::ApiKey);

                // 解析 oauth_config
                let oauth: Option<OAuthConfig> =
                    oauth_config_str.and_then(|s| serde_json::from_str(&s).ok());

                // 解析 oauth_tokens
                let oauth_tokens: Option<OAuthTokens> =
                    oauth_tokens_str.and_then(|s| serde_json::from_str(&s).ok());

                // 解析 headers
                let headers: HashMap<String, String> = headers_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();

                Ok(Provider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    prefix: row.get(2)?,
                    base_url: row.get(3)?,
                    api_key: row.get(4)?,
                    api_format: serde_json::from_str(&api_format_str).unwrap_or(ApiFormat::OpenAI),
                    models,
                    enable_cost: row
                        .get::<_, Option<i32>>(7)?
                        .map(|v| v != 0)
                        .unwrap_or(false),
                    currency,
                    auth_type,
                    oauth,
                    oauth_tokens,
                    headers,
                    auth_header: auth_header.unwrap_or_else(|| "Authorization".to_string()),
                    auth_prefix,
                    is_active: row.get::<_, i32>(15)? != 0,
                    is_builtin: row.get::<_, i32>(16)? != 0,
                    created_at: parse_dt(&created_str),
                    updated_at: parse_dt(&updated_str),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(providers)
    }

    pub fn get_provider_by_prefix(&self, prefix: &str) -> Result<Option<Provider>> {
        let db = self.conn.lock().unwrap();
        let result = db.query_row(
            "SELECT id, name, prefix, base_url, api_key, api_format, models, enable_cost, currency, auth_type, oauth_config, oauth_tokens, headers, auth_header, auth_prefix, is_active, is_builtin, created_at, updated_at
             FROM providers WHERE prefix=?1",
            [prefix],
            |row| {
                let api_format_str: String = row.get(5)?;
                let models_str: String = row.get(6)?;
                let currency: String = row.get(8).unwrap_or_else(|_| "CNY".to_string());
                let auth_type_str: Option<String> = row.get(9)?;
                let oauth_config_str: Option<String> = row.get(10)?;
                let oauth_tokens_str: Option<String> = row.get(11)?;
                let headers_str: Option<String> = row.get(12)?;
                let auth_header: Option<String> = row.get(13)?;
                let auth_prefix: Option<String> = row.get(14)?;
                let created_str: String = row.get(17)?;
                let updated_str: String = row.get(18)?;

                // 尝试解析新格式的 models，如果失败则尝试旧格式
                let models: Vec<crate::models::ModelConfig> = if models_str.starts_with('[') && models_str.contains("\"name\"") {
                    serde_json::from_str(&models_str).unwrap_or_default()
                } else {
                    let old_models: Vec<String> = serde_json::from_str(&models_str).unwrap_or_default();
                    old_models.into_iter().map(|name| crate::models::ModelConfig::from(name)).collect()
                };

                let auth_type: AuthType = auth_type_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(AuthType::ApiKey);

                let oauth: Option<OAuthConfig> = oauth_config_str
                    .and_then(|s| serde_json::from_str(&s).ok());

                let oauth_tokens: Option<OAuthTokens> = oauth_tokens_str
                    .and_then(|s| serde_json::from_str(&s).ok());

                let headers: HashMap<String, String> = headers_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();

                Ok(Provider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    prefix: row.get(2)?,
                    base_url: row.get(3)?,
                    api_key: row.get(4)?,
                    api_format: serde_json::from_str(&api_format_str).unwrap_or(ApiFormat::OpenAI),
                    models,
                    enable_cost: row.get::<_, Option<i32>>(7)?.map(|v| v != 0).unwrap_or(false),
                    currency,
                    auth_type,
                    oauth,
                    oauth_tokens,
                    headers,
                    auth_header: auth_header.unwrap_or_else(|| "Authorization".to_string()),
                    auth_prefix,
                    is_active: row.get::<_, i32>(15)? != 0,
                    is_builtin: row.get::<_, i32>(16)? != 0,
                    created_at: parse_dt(&created_str),
                    updated_at: parse_dt(&updated_str),
                })
            },
        ).optional()?;
        Ok(result)
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM providers WHERE id=?1 AND is_builtin=0", [id])?;
        Ok(())
    }

    // ============ Groups ============

    pub fn save_group(&self, group: &Group) -> Result<()> {
        let db = self.conn.lock().unwrap();
        db.execute(
            r#"INSERT INTO groups (id, name, description, strategy, config, is_active, created_at, updated_at, endpoint_type)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
               ON CONFLICT(id) DO UPDATE SET
                 name=?2, description=?3, strategy=?4, config=?5, is_active=?6, updated_at=?8, endpoint_type=?9"#,
            rusqlite::params![
                group.id, group.name, group.description,
                serde_json::to_string(&group.strategy)?,
                serde_json::to_string(&group.config)?,
                group.is_active as i32,
                group.created_at.to_rfc3339(),
                group.updated_at.to_rfc3339(),
                group.endpoint_type,
            ],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                anyhow::anyhow!("该端点类型下已存在同名组合")
            } else {
                anyhow::anyhow!("数据库错误: {}", e)
            }
        })?;

        db.execute("DELETE FROM group_models WHERE group_id=?1", [&group.id])?;

        for m in &group.models {
            db.execute(
                "INSERT INTO group_models (group_id, model, weight, priority, enabled) VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![group.id, m.model, m.weight, m.priority, m.enabled as i32],
            )?;
        }

        Ok(())
    }

    pub fn load_groups(&self) -> Result<Vec<Group>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, name, description, strategy, config, is_active, created_at, updated_at, endpoint_type FROM groups ORDER BY name"
        )?;
        let groups = stmt
            .query_map([], |row| {
                let strategy_str: String = row.get(3)?;
                let config_str: String = row.get(4)?;
                let created_str: String = row.get(6)?;
                let updated_str: String = row.get(7)?;
                Ok(Group {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    strategy: serde_json::from_str(&strategy_str).unwrap_or_default(),
                    config: serde_json::from_str(&config_str).unwrap_or_default(),
                    is_active: row.get::<_, i32>(5)? != 0,
                    created_at: parse_dt(&created_str),
                    updated_at: parse_dt(&updated_str),
                    models: Vec::new(),
                    endpoint_type: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut result = groups;
        for group in &mut result {
            let mut model_stmt = db.prepare(
                "SELECT model, weight, priority, enabled FROM group_models WHERE group_id=?1 ORDER BY priority"
            )?;
            group.models = model_stmt
                .query_map([&group.id], |row| {
                    Ok(GroupModel {
                        model: row.get(0)?,
                        weight: row.get(1)?,
                        priority: row.get(2)?,
                        enabled: row.get::<_, i32>(3)? != 0,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
        }

        Ok(result)
    }

    pub fn delete_group(&self, id: &str) -> Result<()> {
        let db = self.conn.lock().unwrap();
        db.execute("DELETE FROM group_models WHERE group_id=?1", [id])?;
        db.execute("DELETE FROM groups WHERE id=?1", [id])?;
        Ok(())
    }

    // ============ Model Mappings ============

    pub fn save_model_mapping(&self, m: &ModelMapping) -> Result<()> {
        let db = self.conn.lock().unwrap();
        db.execute(
            r#"INSERT INTO model_mappings (id, pattern, group_id, priority)
               VALUES (?1,?2,?3,?4)
               ON CONFLICT(id) DO UPDATE SET pattern=?2, group_id=?3, priority=?4"#,
            rusqlite::params![m.id, m.pattern, m.group_id, m.priority],
        )?;
        Ok(())
    }

    pub fn load_model_mappings(&self) -> Result<Vec<ModelMapping>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, pattern, group_id, priority FROM model_mappings ORDER BY priority",
        )?;
        let mappings = stmt
            .query_map([], |row| {
                Ok(ModelMapping {
                    id: row.get(0)?,
                    pattern: row.get(1)?,
                    group_id: row.get(2)?,
                    priority: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(mappings)
    }

    pub fn delete_model_mapping(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM model_mappings WHERE id=?1", [id])?;
        Ok(())
    }

    // ============ Settings ============

    pub fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1,?2)",
            [key, value],
        )?;
        Ok(())
    }

    pub fn load_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    // ============ Export/Import ============

    pub fn export_json(&self) -> Result<String> {
        let data = ExportData {
            version: 3,
            exported_at: chrono::Utc::now().to_rfc3339(),
            providers: self.load_providers()?,
            groups: self.load_groups()?,
            model_mappings: self.load_model_mappings()?,
        };
        Ok(serde_json::to_string_pretty(&data)?)
    }

    pub fn import_json(&self, json: &str, merge: bool) -> Result<ImportResult> {
        let data: ExportData = serde_json::from_str(json)?;
        let mut result = ImportResult::default();

        if !merge {
            let db = self.conn.lock().unwrap();
            db.execute("DELETE FROM group_models", [])?;
            db.execute("DELETE FROM model_mappings", [])?;
            db.execute("DELETE FROM groups", [])?;
            db.execute("DELETE FROM providers WHERE is_builtin=0", [])?;
        }

        for provider in data.providers {
            if !provider.is_builtin {
                match self.save_provider(&provider) {
                    Ok(_) => result.providers_imported += 1,
                    Err(e) => result
                        .errors
                        .push(format!("供应商 {} 导入失败: {}", provider.name, e)),
                }
            }
        }

        for group in data.groups {
            match self.save_group(&group) {
                Ok(_) => result.groups_imported += 1,
                Err(e) => result
                    .errors
                    .push(format!("Group {} 导入失败: {}", group.name, e)),
            }
        }

        for mapping in data.model_mappings {
            match self.save_model_mapping(&mapping) {
                Ok(_) => result.mappings_imported += 1,
                Err(e) => result
                    .errors
                    .push(format!("映射 {} 导入失败: {}", mapping.pattern, e)),
            }
        }

        Ok(result)
    }

    pub fn path(&self) -> &PathBuf {
        &self.db_path
    }

    // ============ Request Logs ============

    pub fn save_request_log(&self, log: &RequestLog) -> Result<i64> {
        let db = self.conn.lock().unwrap();
        db.execute(
            r#"INSERT INTO request_logs
               (timestamp, method, path, requested_model, model, provider, provider_prefix, url, protocol_transform, endpoint_type, status_code, latency_ms, first_token_ms, prompt_tokens, completion_tokens, original_input_tokens, translated_input_tokens, cost, error, original_request_body, request_body, original_response_body, response_body)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)"#,
            rusqlite::params![
                log.timestamp.to_rfc3339(),
                log.method,
                log.path,
                log.requested_model,
                log.model,
                log.provider,
                log.provider_prefix,
                log.url,
                log.protocol_transform,
                log.endpoint_type,
                log.status_code,
                log.latency_ms,
                log.first_token_ms,
                log.prompt_tokens,
                log.completion_tokens,
                log.original_input_tokens,
                log.translated_input_tokens,
                log.cost,
                log.error,
                log.original_request_body,
                log.request_body,
                log.original_response_body,
                log.response_body,
            ],
        )?;
        Ok(db.last_insert_rowid())
    }

    pub fn load_request_logs(&self, limit: i64, offset: i64) -> Result<Vec<RequestLog>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, timestamp, method, path, requested_model, model, provider, provider_prefix, url, protocol_transform, endpoint_type, status_code, latency_ms, first_token_ms, prompt_tokens, completion_tokens, original_input_tokens, translated_input_tokens, cost, error, original_request_body, request_body, original_response_body, response_body
             FROM request_logs ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2"
        )?;
        let logs = stmt
            .query_map([limit, offset], |row| {
                let timestamp_str: String = row.get(1)?;
                Ok(RequestLog {
                    id: row.get(0)?,
                    timestamp: parse_dt(&timestamp_str),
                    method: row.get(2)?,
                    path: row.get(3)?,
                    requested_model: row.get(4)?,
                    model: row.get(5)?,
                    provider: row.get(6)?,
                    provider_prefix: row.get(7)?,
                    url: row.get(8)?,
                    protocol_transform: row.get(9)?,
                    endpoint_type: row.get(10)?,
                    status_code: row.get(11)?,
                    latency_ms: row.get(12)?,
                    first_token_ms: row.get(13)?,
                    prompt_tokens: row.get(14)?,
                    completion_tokens: row.get(15)?,
                    original_input_tokens: row.get(16)?,
                    translated_input_tokens: row.get(17)?,
                    cost: row.get(18)?,
                    error: row.get(19)?,
                    original_request_body: row.get(20)?,
                    request_body: row.get(21)?,
                    original_response_body: row.get(22)?,
                    response_body: row.get(23)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    pub fn count_request_logs(&self) -> Result<i64> {
        let db = self.conn.lock().unwrap();
        let count: i64 = db.query_row("SELECT COUNT(*) FROM request_logs", [], |r| r.get(0))?;
        Ok(count)
    }

    pub fn clear_request_logs(&self) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM request_logs", [])?;
        Ok(())
    }

    pub fn get_stats(&self) -> Result<RequestStats> {
        let db = self.conn.lock().unwrap();
        let total_requests: i64 =
            db.query_row("SELECT COUNT(*) FROM request_logs", [], |r| r.get(0))?;
        let successful_requests: i64 = db.query_row(
            "SELECT COUNT(*) FROM request_logs WHERE status_code >= 200 AND status_code < 300",
            [],
            |r| r.get(0),
        )?;
        let failed_requests: i64 = db.query_row(
            "SELECT COUNT(*) FROM request_logs WHERE status_code >= 400 OR error IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        let total_tokens: i64 = db.query_row(
            "SELECT COALESCE(SUM(COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0)), 0) FROM request_logs",
            [], |r| r.get(0)
        )?;
        let total_cost: f64 =
            db.query_row("SELECT COALESCE(SUM(cost), 0) FROM request_logs", [], |r| {
                r.get(0)
            })?;
        let avg_latency: f64 = db.query_row(
            "SELECT COALESCE(AVG(latency_ms), 0) FROM request_logs",
            [],
            |r| r.get(0),
        )?;

        Ok(RequestStats {
            total_requests,
            successful_requests,
            failed_requests,
            total_tokens,
            total_cost,
            avg_latency_ms: avg_latency,
        })
    }

    /// 获取按模型分组的成本统计
    pub fn get_cost_by_model(&self, limit: i64) -> Result<Vec<CostByModel>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            r#"SELECT model,
                      COUNT(*) as request_count,
                      SUM(COALESCE(prompt_tokens, 0)) as total_prompt_tokens,
                      SUM(COALESCE(completion_tokens, 0)) as total_completion_tokens,
                      SUM(COALESCE(cost, 0)) as total_cost
               FROM request_logs
               WHERE model IS NOT NULL
               GROUP BY model
               ORDER BY total_cost DESC
               LIMIT ?1"#,
        )?;
        let stats = stmt
            .query_map([limit], |row| {
                Ok(CostByModel {
                    model: row.get(0)?,
                    request_count: row.get(1)?,
                    total_prompt_tokens: row.get(2)?,
                    total_completion_tokens: row.get(3)?,
                    total_cost: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(stats)
    }

    /// 获取按供应商分组的成本统计
    pub fn get_cost_by_provider(&self, limit: i64) -> Result<Vec<CostByProvider>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            r#"SELECT provider,
                      COUNT(*) as request_count,
                      SUM(COALESCE(prompt_tokens, 0)) as total_prompt_tokens,
                      SUM(COALESCE(completion_tokens, 0)) as total_completion_tokens,
                      SUM(COALESCE(cost, 0)) as total_cost
               FROM request_logs
               WHERE provider IS NOT NULL
               GROUP BY provider
               ORDER BY total_cost DESC
               LIMIT ?1"#,
        )?;
        let stats = stmt
            .query_map([limit], |row| {
                Ok(CostByProvider {
                    provider: row.get(0)?,
                    request_count: row.get(1)?,
                    total_prompt_tokens: row.get(2)?,
                    total_completion_tokens: row.get(3)?,
                    total_cost: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(stats)
    }

    /// 获取每日成本统计
    pub fn get_daily_cost(&self, days: i64) -> Result<Vec<DailyCost>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            r#"SELECT date(timestamp) as date,
                      COUNT(*) as request_count,
                      SUM(COALESCE(prompt_tokens, 0)) as total_prompt_tokens,
                      SUM(COALESCE(completion_tokens, 0)) as total_completion_tokens,
                      SUM(COALESCE(cost, 0)) as total_cost
               FROM request_logs
               WHERE timestamp >= datetime('now', '-' || ?1 || ' days')
               GROUP BY date(timestamp)
               ORDER BY date DESC"#,
        )?;
        let stats = stmt
            .query_map([days], |row| {
                Ok(DailyCost {
                    date: row.get(0)?,
                    request_count: row.get(1)?,
                    total_prompt_tokens: row.get(2)?,
                    total_completion_tokens: row.get(3)?,
                    total_cost: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(stats)
    }
}

/// 按模型分组的成本统计
#[derive(Debug, Clone, serde::Serialize)]
pub struct CostByModel {
    pub model: String,
    pub request_count: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_cost: f64,
}

/// 按供应商分组的成本统计
#[derive(Debug, Clone, serde::Serialize)]
pub struct CostByProvider {
    pub provider: String,
    pub request_count: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_cost: f64,
}

/// 每日成本统计
#[derive(Debug, Clone, serde::Serialize)]
pub struct DailyCost {
    pub date: String,
    pub request_count: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_cost: f64,
}

impl Database {
    /// 获取今日成本
    pub fn get_today_cost(&self) -> Result<f64> {
        let db = self.conn.lock().unwrap();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let cost: f64 = db.query_row(
            "SELECT COALESCE(SUM(cost), 0) FROM request_logs WHERE date(timestamp) = ?1",
            [&today],
            |r| r.get(0),
        )?;
        Ok(cost)
    }

    /// 获取本月成本
    pub fn get_month_cost(&self) -> Result<f64> {
        let db = self.conn.lock().unwrap();
        let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();
        let cost: f64 = db.query_row(
            "SELECT COALESCE(SUM(cost), 0) FROM request_logs WHERE date(timestamp) >= ?1",
            [&month_start],
            |r| r.get(0),
        )?;
        Ok(cost)
    }

    /// 获取最近24小时流量统计
    pub fn get_hourly_traffic(&self, hours: i64) -> Result<Vec<HourlyTraffic>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            r#"SELECT 
                strftime('%Y-%m-%d %H:00', timestamp) as hour,
                COUNT(*) as request_count,
                SUM(COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0)) as total_tokens,
                SUM(COALESCE(cost, 0)) as total_cost
               FROM request_logs
               WHERE timestamp >= datetime('now', '-' || ?1 || ' hours')
               GROUP BY strftime('%Y-%m-%d %H', timestamp)
               ORDER BY hour ASC"#,
        )?;
        let stats = stmt
            .query_map([hours], |row| {
                Ok(HourlyTraffic {
                    hour: row.get(0)?,
                    request_count: row.get(1)?,
                    total_tokens: row.get(2)?,
                    total_cost: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(stats)
    }

    /// 获取供应商健康状态
    pub fn get_provider_health(&self, hours: i64) -> Result<Vec<ProviderHealth>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare(
            r#"SELECT 
                COALESCE(provider, '未知') as provider,
                COALESCE(provider_prefix, 'unknown') as provider_prefix,
                COUNT(*) as request_count,
                SUM(CASE WHEN status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END) as success_count,
                SUM(CASE WHEN status_code >= 400 OR status_code = 0 THEN 1 ELSE 0 END) as failed_count,
                AVG(CASE WHEN status_code >= 200 AND status_code < 300 THEN latency_ms ELSE NULL END) as avg_latency_ms,
                SUM(COALESCE(cost, 0)) as total_cost
               FROM request_logs
               WHERE timestamp >= datetime('now', '-' || ?1 || ' hours')
               GROUP BY COALESCE(provider_prefix, 'unknown')
               ORDER BY request_count DESC"#,
        )?;
        let stats = stmt
            .query_map([hours], |row| {
                let request_count: i64 = row.get(2)?;
                let success_count: i64 = row.get(3)?;
                let failed_count: i64 = row.get(4)?;
                let success_rate = if request_count > 0 {
                    (success_count as f64 / request_count as f64) * 100.0
                } else {
                    0.0
                };
                Ok(ProviderHealth {
                    provider: row.get(0)?,
                    provider_prefix: row.get(1)?,
                    request_count,
                    success_count,
                    failed_count,
                    success_rate,
                    avg_latency_ms: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                    total_cost: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(stats)
    }
}

fn parse_dt(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExportData {
    pub version: u32,
    pub exported_at: String,
    pub providers: Vec<Provider>,
    pub groups: Vec<Group>,
    pub model_mappings: Vec<ModelMapping>,
}

#[derive(Default)]
pub struct ImportResult {
    pub providers_imported: usize,
    pub groups_imported: usize,
    pub mappings_imported: usize,
    pub errors: Vec<String>,
}

/// 按小时流量统计
#[derive(Debug, Clone, serde::Serialize)]
pub struct HourlyTraffic {
    pub hour: String,
    pub request_count: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
}

/// 供应商健康状态
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderHealth {
    pub provider: String,
    pub provider_prefix: String,
    pub request_count: i64,
    pub success_count: i64,
    pub failed_count: i64,
    pub success_rate: f64,
    pub avg_latency_ms: f64,
    pub total_cost: f64,
}
