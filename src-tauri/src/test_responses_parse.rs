use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: ResponsesInput,
    #[serde(default)] pub stream: bool,
    #[serde(default)] pub tools: Vec<serde_json::Value>,
    #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    Text(String),
    Items(Vec<serde_json::Value>),
    Raw(serde_json::Value),
}

fn main() {
    // Codex 发送的格式
    let json = r#"{"messages":[{"content":[{"text":"hello","type":"text"}],"role":"user"}],"model":"glm-5","stream":true,"tools":[]}"#;
    println!("Input: {}", json);
    
    match serde_json::from_str::<ResponsesRequest>(json) {
        Ok(req) => println!("Parsed: {:?}", req),
        Err(e) => println!("Parse error: {}", e),
    }
}
