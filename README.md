# UniRoute

**统一 AI 路由器** - 将多个 AI 提供商统一为一个接口

## 核心特性

- **模型别名映射**: 配置 `gpt-4` → [OpenAI官方, API2D中转, Azure]，自动故障转移
- **多协议支持**: 自动识别 OpenAI、Claude、Gemini 协议格式
- **协议转换**: 请求统一为 OpenAI 格式，响应按需转换
- **智能路由**: 支持优先级、轮询、随机、最低延迟等策略
- **故障转移**: 自动重试和连接切换

## 架构简化

相比原 OmniRoute 的路由+组合分离设计，UniRoute 采用更直观的模型映射方案：

```
请求 model="gpt-4" → 查找映射 → 选择连接 → 自动协议转换 → 执行
```

**配置示例：**
```
gpt-4 → [
  { connection: "OpenAI官方", target_model: "gpt-4-turbo", priority: 0 },
  { connection: "API2D中转", target_model: "gpt-4", priority: 1 },
  { connection: "Azure", target_model: "gpt-4", priority: 2 }
]
```

## 快速开始

### 前置要求

- Node.js 18+
- Rust 1.70+

### 安装和运行

```bash
cd omniroute-tauri

# 安装依赖
npm install

# 开发模式
npm run tauri:dev

# 构建
npm run tauri:build
```

## 使用方式

### 1. 添加连接

在"连接"页面添加你的 AI 服务连接：
- 支持 API Key 和 OAuth 认证
- 自动检测支持的协议

### 2. 创建模型映射

在"模型映射"页面配置路由规则：
- 设置模型别名（如 `gpt-4`、`claude-3-opus`）
- 添加多个连接作为备份
- 配置路由策略和故障转移

### 3. 启动代理

启动代理后，所有请求发送到 `http://localhost:8080/v1/chat/completions`

### 4. 直接指定连接

支持 `provider/model` 格式直接指定连接：
```json
{
  "model": "openai-official/gpt-4",
  "messages": [...]
}
```

## 项目结构

```
uniroute/
├── src/                      # React 前端
│   ├── pages/
│   │   ├── Dashboard.tsx     # 仪表盘
│   │   ├── Providers.tsx     # 连接管理
│   │   ├── Mappings.tsx      # 模型映射
│   │   └── Settings.tsx      # 设置
│   └── i18n/                 # 国际化
├── src-tauri/                # Rust 后端
│   ├── src/
│   │   ├── models/           # 数据模型
│   │   ├── commands/         # Tauri 命令
│   │   ├── proxy/            # 代理服务器
│   │   ├── router/           # 路由逻辑
│   │   ├── translator/       # 协议转换
│   │   └── state/            # 状态管理
│   └── Cargo.toml
└── package.json
```

## API 端点

代理服务器提供以下端点：

- `POST /v1/chat/completions` - OpenAI 兼容聊天接口
- `POST /v1/messages` - Claude 兼容消息接口
- `POST /v1/embeddings` - 嵌入接口
- `GET /v1/models` - 模型列表

## 协议转换

支持的协议转换：
- OpenAI ↔ Claude
- OpenAI ↔ Gemini
- Claude ↔ Gemini

流式响应（SSE）转换也已支持。

## 技术栈

- **前端**: React 18 + TypeScript + Tailwind CSS
- **后端**: Tauri 2 + Rust
- **代理**: Axum
- **存储**: SQLite (计划中)

## 许可证

MIT
