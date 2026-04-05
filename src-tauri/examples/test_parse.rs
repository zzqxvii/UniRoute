use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    #[serde(default)]
    pub input: Option<ResponsesInput>,
    #[serde(default)] pub stream: bool,
    #[serde(default)] pub instructions: Option<String>,
    #[serde(default)] pub tools: Vec<serde_json::Value>,
    #[serde(default)] pub temperature: Option<f64>,
    #[serde(default)] pub max_output_tokens: Option<i32>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    Text(String),
    Items(Vec<serde_json::Value>),
    Raw(serde_json::Value),
}

fn main() {
    // Codex 发送的格式（没有 input 字段，有 messages 字段）
    let json = r#"{"messages":[{"content":[{"text":"hello","type":"text"}],"role":"user"}],"model":"glm-5","stream":true,"tools":[]}"#;
    println!("=== Test 1: Codex format ===");
    println!("Input: {}", json);
    
    match serde_json::from_str::<ResponsesRequest>(json) {
        Ok(req) => {
            println!("Parsed successfully!");
            println!("  model: {}", req.model);
            println!("  input: {:?}", req.input);
            println!("  extra keys: {:?}", req.extra.keys().collect::<Vec<_>>());
            if let Some(messages) = req.extra.get("messages") {
                println!("  messages found: {:?}", messages);
            }
        }
        Err(e) => println!("Parse error: {}", e),
    }
    
    // 标准 Responses API 格式
    let json2 = r#"{"input":[{"type":"message","role":"user","content":"hello"}],"model":"glm-5"}"#;
    println!("\n=== Test 2: Standard Responses format ===");
    println!("Input: {}", json2);
    
    match serde_json::from_str::<ResponsesRequest>(json2) {
        Ok(req) => {
            println!("Parsed successfully!");
            println!("  model: {}", req.model);
            println!("  input: {:?}", req.input);
        }
        Err(e) => println!("Parse error: {}", e),
    }
}
