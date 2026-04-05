use serde_json;

fn main() {
    let json = r#"{"messages":[{"content":[{"text":"hello","type":"text"}],"role":"user"}],"model":"glm-5","stream":true,"tools":[]}"#;
    println!("Input JSON: {}", json);
    
    // 测试解析为 ResponsesRequest
    let result: Result<crate::models::ResponsesRequest, _> = serde_json::from_str(json);
    println!("Parse result: {:?}", result);
}
