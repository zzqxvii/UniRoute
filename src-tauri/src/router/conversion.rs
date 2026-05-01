//! 请求/响应格式转换模块
//!
//! Responses API 与 Chat API 之间的格式互转。

use crate::models::{
    ChatRequest, ContentPart, FunctionCall, ImageUrl, Message, MessageContent, ResponsesRequest,
    ResponsesContent, ResponsesInput, Tool, ToolCall,
};

/// 将 Responses API 请求转换为 Chat API 请求
/// 支持：
/// 1. 标准 Responses API 格式：{"input": ...}
/// 2. Codex 格式（Chat API 风格）：{"messages": [...]}
pub fn responses_to_chat_request(responses_req: &ResponsesRequest) -> ChatRequest {
    // 首先检查是否是 Codex 格式（extra 中有 messages 字段）
    if let Some(messages_value) = responses_req.extra.get("messages") {
        if let Some(messages_array) = messages_value.as_array() {
            tracing::info!("检测到 Codex 格式的 Responses 请求（使用 messages 字段）");
            let messages: Vec<Message> = messages_array
                .iter()
                .map(|msg| {
                    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    let content = msg.get("content");

                    let msg_content = match content {
                        Some(c) if c.is_string() => {
                            MessageContent::Text(c.as_str().unwrap_or("").to_string())
                        }
                        Some(c) if c.is_array() => {
                            // 处理 Codex 格式的 content 数组: [{"text": "...", "type": "text"}]
                            // 也处理 Responses API 格式: [{"type": "input_text", "text": "..."}]
                            let parts: Vec<ContentPart> = c
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter_map(|part| {
                                    let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                                    let text = part.get("text").and_then(|t| t.as_str());

                                    // 支持 "text"、"input_text"、"output_text" 类型
                                    if part_type == "text" || part_type == "input_text" || part_type == "output_text" {
                                        text.map(|t| ContentPart {
                                            content_type: "text".to_string(),
                                            text: Some(t.to_string()),
                                            image_url: None,
                                            extra: serde_json::Map::new(),
                                        })
                                    } else if part_type == "image_url" {
                                        part.get("image_url").map(|img_url| ContentPart {
                                            content_type: "image_url".to_string(),
                                            text: None,
                                            image_url: Some(ImageUrl {
                                                url: img_url.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                                                detail: None,
                                            }),
                                            extra: serde_json::Map::new(),
                                        })
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            MessageContent::Parts(parts)
                        }
                        _ => MessageContent::Text(String::new()),
                    };

                    Message {
                        role: role.to_string(),
                        content: msg_content,
                        name: None,
                        tool_calls: Vec::new(),
                        tool_call_id: None,
                    }
                })
                .collect();

            let tools: Vec<Tool> = responses_req.tools.iter()
                .filter(|t| t.tool_type == "function" || t.function.is_some())
                .map(|t| {
                    let normalized = t.normalize();
                    tracing::debug!(
                        "Tool normalize: before tool_type={}, function={:?}, after tool_type={}, function={:?}",
                        t.tool_type, t.function, normalized.tool_type, normalized.function
                    );
                    normalized
                })
                .collect();

            tracing::info!("Normalized tools count: {}, first tool: {:?}", tools.len(), tools.first());

            return ChatRequest {
                model: responses_req.model.clone(),
                messages,
                stream: responses_req.stream,
                temperature: responses_req.temperature.map(|t| t as f32),
                top_p: responses_req.top_p.map(|t| t as f32),
                max_tokens: responses_req.max_output_tokens.map(|t| t as u32),
                tools,
                tool_choice: responses_req.tool_choice.clone(),
                parallel_tool_calls: responses_req.parallel_tool_calls,
                response_format: responses_req.response_format.clone(),
                user: responses_req.user.clone(),
                n: None,
                stop: None,
                presence_penalty: None,
                frequency_penalty: None,
                logit_bias: None,
                seed: None,
                logprobs: None,
                top_logprobs: None,
                extra: responses_req.extra.clone(),
            };
        }
    }

    // 标准 Responses API 格式
    let messages = match &responses_req.input {
        Some(ResponsesInput::Text(text)) => {
            vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text(text.clone()),
                name: None,
                tool_calls: Vec::new(),
                tool_call_id: None,
            }]
        }
        Some(ResponsesInput::Items(items)) => {
            let mut messages: Vec<Message> = Vec::new();

            for item in items {
                match item.item_type.as_str() {
                    "message" => {
                        // 获取角色，处理特殊角色映射
                        let role = item.role.clone().unwrap_or_else(|| "user".to_string());

                        // 将 developer 角色映射为 system
                        let normalized_role = match role.as_str() {
                            "developer" => "system".to_string(),
                            other => other.to_string(),
                        };

                        // 只处理有效的角色
                        if normalized_role != "system" && normalized_role != "user" && normalized_role != "assistant" {
                            tracing::debug!("跳过无效角色: {}", normalized_role);
                            continue;
                        }

                        // 转换内容
                        let content = match &item.content {
                            ResponsesContent::Text(text) => {
                                MessageContent::Text(text.clone())
                            }
                            ResponsesContent::Parts(parts) => {
                                let converted: Vec<ContentPart> = parts
                                    .iter()
                                    .filter_map(|p| {
                                        // 转换 Responses API 类型到标准 OpenAI 类型
                                        let normalized_type = match p.content_type.as_str() {
                                            "input_text" | "output_text" => "text",
                                            "input_image" => "image_url",
                                            "refusal" => return None, // 跳过 refusal 类型
                                            other => other,
                                        };

                                        if normalized_type == "text" {
                                            p.text.as_ref().map(|text_content| ContentPart {
                                                content_type: "text".to_string(),
                                                text: Some(text_content.clone()),
                                                image_url: None,
                                                extra: serde_json::Map::new(),
                                            })
                                        } else if normalized_type == "image_url" {
                                            p.image_url.as_ref().map(|img_url| ContentPart {
                                                content_type: "image_url".to_string(),
                                                text: None,
                                                image_url: Some(img_url.clone()),
                                                extra: serde_json::Map::new(),
                                            })
                                        } else {
                                            Some(ContentPart {
                                                content_type: normalized_type.to_string(),
                                                text: p.text.clone(),
                                                image_url: p.image_url.clone(),
                                                extra: serde_json::Map::new(),
                                            })
                                        }
                                    })
                                    .collect();
                                MessageContent::Parts(converted)
                            }
                        };

                        messages.push(Message {
                            role: normalized_role,
                            content,
                            name: None,
                            tool_calls: Vec::new(),
                            tool_call_id: None,
                        });
                    }
                    "function_call" => {
                        // function_call 转换为 assistant message 的 tool_calls
                        let call_id = item.extra.get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = item.extra.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = item.extra.get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}")
                            .to_string();

                        // 检查最后一条消息是否是 assistant，如果是则追加 tool_call
                        if let Some(last_msg) = messages.last_mut() {
                            if last_msg.role == "assistant" {
                                last_msg.tool_calls.push(ToolCall {
                                    id: call_id,
                                    call_type: "function".to_string(),
                                    function: FunctionCall {
                                        name,
                                        arguments,
                                    },
                                });
                                continue;
                            }
                        }

                        // 否则创建新的 assistant message
                        messages.push(Message {
                            role: "assistant".to_string(),
                            content: MessageContent::Text(String::new()),
                            name: None,
                            tool_calls: vec![ToolCall {
                                id: call_id,
                                call_type: "function".to_string(),
                                function: FunctionCall {
                                    name,
                                    arguments,
                                },
                            }],
                            tool_call_id: None,
                        });
                    }
                    "function_call_output" => {
                        // function_call_output 转换为 tool role message
                        let call_id = item.extra.get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let output = item.extra.get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        messages.push(Message {
                            role: "tool".to_string(),
                            content: MessageContent::Text(output),
                            name: None,
                            tool_calls: Vec::new(),
                            tool_call_id: Some(call_id),
                        });
                    }
                    "reasoning" => {
                        // reasoning 类型无法映射到 Chat API，记录日志后跳过
                        tracing::debug!("跳过 reasoning 类型的 input 项");
                    }
                    other => {
                        tracing::warn!("未知的 input 类型: {}", other);
                    }
                }
            }
            messages
        }
        Some(ResponsesInput::Raw(value)) => {
            // 尝试从原始值中提取信息
            tracing::warn!("Responses API 收到未知的 input 格式，尝试转换: {:?}", value);
            // 如果是对象，尝试提取内容作为文本
            if let Some(text) = value.as_str() {
                vec![Message {
                    role: "user".to_string(),
                    content: MessageContent::Text(text.to_string()),
                    name: None,
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                }]
            } else {
                // 无法解析，使用 JSON 字符串
                vec![Message {
                    role: "user".to_string(),
                    content: MessageContent::Text(value.to_string()),
                    name: None,
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                }]
            }
        }
        None => {
            // 没有 input 字段，也没有 messages 字段，使用空消息
            tracing::warn!("Responses API 请求没有 input 或 messages 字段");
            vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text(String::new()),
                name: None,
                tool_calls: Vec::new(),
                tool_call_id: None,
            }]
        }
    };

    // 处理 instructions 字段：添加到 messages 开头作为 system 消息
    let final_messages = if let Some(ref instructions) = responses_req.instructions {
        if !instructions.is_empty() {
            let mut msgs = Vec::with_capacity(messages.len() + 1);
            msgs.push(Message {
                role: "system".to_string(),
                content: MessageContent::Text(instructions.clone()),
                name: None,
                tool_calls: Vec::new(),
                tool_call_id: None,
            });
            msgs.extend(messages);
            msgs
        } else {
            messages
        }
    } else {
        messages
    };

    let tools: Vec<Tool> = responses_req.tools.iter()
        .filter(|t| t.tool_type == "function" || t.function.is_some())
        .map(|t| t.normalize())
        .collect();

    ChatRequest {
        model: responses_req.model.clone(),
        messages: final_messages,
        stream: responses_req.stream,
        // 直接映射的参数
        temperature: responses_req.temperature.map(|t| t as f32),
        top_p: responses_req.top_p.map(|t| t as f32),
        max_tokens: responses_req.max_output_tokens.map(|t| t as u32),
        tools,
        tool_choice: responses_req.tool_choice.clone(),
        parallel_tool_calls: responses_req.parallel_tool_calls,
        response_format: responses_req.response_format.clone(),
        user: responses_req.user.clone(),
        // Chat API 独有参数（Responses API 不支持，保持默认）
        n: None,
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
        logit_bias: None,
        seed: None,
        logprobs: None,
        top_logprobs: None,
        // 其他未知字段
        extra: responses_req.extra.clone(),
    }
}

/// 将 Chat API 响应转换为 Responses API 响应
pub fn chat_to_responses_response(chat_resp: &serde_json::Value, requested_model: &str) -> serde_json::Value {
    // 生成 Responses API 格式的 id
    let original_id = chat_resp.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let response_id = if original_id.starts_with("resp_") {
        original_id.to_string()
    } else {
        format!("resp_{}", &uuid::Uuid::new_v4().to_string().replace("-", "")[..24])
    };

    let model = chat_resp.get("model").and_then(|v| v.as_str()).unwrap_or(requested_model);

    // 提取 choices
    let choices = chat_resp.get("choices").and_then(|c| c.as_array());
    let first_choice = choices.and_then(|arr| arr.first());

    // 提取 message
    let message = first_choice.and_then(|c| c.get("message"));

    // 提取 content（可能为 null 或字符串）
    let content_text = message
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    // 提取 tool_calls（如果存在）
    let tool_calls = message.and_then(|m| m.get("tool_calls")).and_then(|tc| tc.as_array());

    // 提取 finish_reason
    let finish_reason = first_choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str());

    // 确定 status
    let status = match finish_reason {
        Some("stop") => "completed",
        Some("length") => "incomplete",
        Some("tool_calls") => "completed",
        _ => "completed",
    };

    // 构建 output 数组
    let mut output: Vec<serde_json::Value> = Vec::new();

    // 添加 message 项（如果有内容）
    if !content_text.is_empty() {
        output.push(serde_json::json!({
            "type": "message",
            "id": format!("msg_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string()),
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": content_text,
                "annotations": []
            }]
        }));
    }

    // 添加 function_call 项（如果有 tool_calls）
    if let Some(calls) = tool_calls {
        for call in calls {
            let call_id = call.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let function = call.get("function");
            let name = function.and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("");
            let arguments = function.and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("{}");

            output.push(serde_json::json!({
                "type": "function_call",
                "id": format!("fc_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string()),
                "call_id": call_id,
                "name": name,
                "arguments": arguments,
                "status": "completed"
            }));
        }
    }

    // 如果 output 为空，添加一个空的 message
    if output.is_empty() {
        output.push(serde_json::json!({
            "type": "message",
            "id": format!("msg_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..24].to_string()),
            "status": "completed",
            "role": "assistant",
            "content": []
        }));
    }

    // 处理 usage
    let usage = chat_resp.get("usage").map(|u| {
        let input_tokens = u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        let output_tokens = u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        serde_json::json!({
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": u.get("total_tokens").and_then(|v| v.as_i64())
                .unwrap_or(input_tokens + output_tokens),
        })
    });

    serde_json::json!({
        "id": response_id,
        "object": "response",
        "created_at": chrono::Utc::now().timestamp(),
        "status": status,
        "error": null,
        "incomplete_details": if status == "incomplete" {
            serde_json::json!({"reason": "max_output_tokens"})
        } else {
            serde_json::Value::Null
        },
        "model": model,
        "output": output,
        "usage": usage,
    })
}

/// 将 Responses API 格式响应转换为 Chat Completions 格式
pub fn responses_to_chat_response(responses_resp: &serde_json::Value, requested_model: &str) -> serde_json::Value {
    let id = responses_resp.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
    let model = requested_model;

    let output = responses_resp.get("output").and_then(|v| v.as_array());
    let mut content = String::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut has_tool_use = false;

    if let Some(items) = output {
        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match item_type {
                "message" => {
                    if let Some(msg_content) = item.get("content").and_then(|c| c.as_array()) {
                        for block in msg_content {
                            let block_type = block.get("type").and_then(|t| t.as_str());
                            if block_type == Some("output_text") || block_type == Some("input_text") {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    content.push_str(text);
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let args = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");

                    tool_calls.push(serde_json::json!({
                        "id": call_id,
                        "type": "function",
                        "function": {"name": name, "arguments": args}
                    }));
                    has_tool_use = true;
                }
                _ => {}
            }
        }
    }

    let status = responses_resp.get("status").and_then(|v| v.as_str()).unwrap_or("completed");
    let finish_reason = match (status, has_tool_use) {
        ("completed", true) => "tool_calls",
        ("completed", false) => "stop",
        ("incomplete", _) => "length",
        _ => "stop",
    };

    let usage = responses_resp.get("usage").cloned().unwrap_or(serde_json::json!({}));
    let prompt_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let completion_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    let message = if !tool_calls.is_empty() {
        serde_json::json!({
            "role": "assistant",
            "content": if content.is_empty() { serde_json::Value::Null } else { serde_json::json!(content) },
            "tool_calls": tool_calls
        })
    } else {
        serde_json::json!({"role": "assistant", "content": content})
    };

    serde_json::json!({
        "id": id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens
        }
    })
}
