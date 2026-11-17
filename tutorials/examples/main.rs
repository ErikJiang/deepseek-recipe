//! 分阶段观察同一条请求。无参数时只运行基础成功路径。
use std::error::Error;
use std::path::Path;

use deepseek_recipe::anthropic::MessagesRequest;
use deepseek_recipe::openai::ChatCompletionRequest;
use deepseek_recipe::request::{ConversationRequest, ConversionOptions, ProtocolRequest};
use deepseek_recipe::response::{Accumulate, ProtocolResponse};
use deepseek_recipe::stream::{
    InferenceChunk, InferenceFinishReason, PromptUsage, StreamProcessor,
};
use deepseek_recipe_encoding::v4::dsv41::DeepseekV41Encoding;
use deepseek_recipe_encoding::{PromptEncoding, PromptTokenizer};
use serde_json::{Value, json};
use tokenizers::Tokenizer;
use tokio_stream::{StreamExt, iter};

type TutorialResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const MODEL: &str = "DeepSeek-V4.1-Flash";

fn request(thinking: Option<bool>, stop: Option<&str>) -> TutorialResult<ConversationRequest> {
    let mut body = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "temperature": 0.3,
        "top_p": 0.8,
        "stream": true
    });
    if let Some(enabled) = thinking {
        body["thinking"] = json!({"type": if enabled { "enabled" } else { "disabled" }});
    }
    if let Some(stop) = stop {
        body["stop"] = json!(stop);
    }
    let request: ChatCompletionRequest = serde_json::from_value(body)?;
    Ok(request.convert(ConversionOptions::default())?)
}

fn tokenizer() -> TutorialResult<Tokenizer> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../static/tokenizers/v41/tokenizer.json");
    Tokenizer::from_file(path)
}

// 每段计 1 是教学用量，不代表真实 tokenizer 的编码结果。
fn texts(parts: &[&str]) -> Vec<InferenceChunk> {
    let mut chunks: Vec<_> = parts
        .iter()
        .map(|text| InferenceChunk::Text {
            content: (*text).into(),
            token_count: 1,
        })
        .collect();
    chunks.push(InferenceChunk::Finish {
        finish_reason: InferenceFinishReason::Stop,
    });
    chunks
}

// 教学实现：完整走过本项目的生成器和累积接口，避免手工拼 JSON 充当验证。
async fn collect_response(
    request: ConversationRequest,
    chunks: Vec<InferenceChunk>,
    tokenizer: Option<Tokenizer>,
    print_events: bool,
) -> TutorialResult<Value> {
    let generator =
        ChatCompletionRequest::chunk_generator(&request, "tutorial-id".into(), MODEL.into());
    let mut processor = StreamProcessor::new(generator, request.parsing_options);
    if let Some(tokenizer) = tokenizer {
        processor = processor.with_tokenizer(tokenizer);
    }
    let mut response = <ChatCompletionRequest as ProtocolRequest>::Response::new(
        "tutorial-id".into(),
        MODEL.into(),
        0,
        0,
        0,
    );
    let mut stream = std::pin::pin!(processor.into_protocol_stream(iter(chunks)));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if print_events {
            println!("协议 chunk = {}", serde_json::to_string(&chunk)?);
        }
        response.accumulate(chunk);
    }
    Ok(serde_json::to_value(response)?)
}

async fn basic(stop: Option<&str>) -> TutorialResult {
    let converted = request(Some(false), stop)?;
    println!("1. model_version = {:?}", converted.model_version);
    println!("   conversation = {:?}", converted.conversation);
    println!("   inference_options = {:?}", converted.inference_options);
    println!("   parsing_options = {:?}", converted.parsing_options);
    let tokenizer = tokenizer()?;
    let encoding = DeepseekV41Encoding::new().with_tokenizer(PromptTokenizer::new(&tokenizer));
    let rendered = encoding.render_conversation(&converted.conversation);
    let ids = encoding.encode(&converted.conversation)?;
    println!("2. prompt = {}", rendered.prompt);
    for range in &rendered.special_ranges {
        println!(
            "   special_range {range:?} = {:?}",
            &rendered.prompt[range.clone()]
        );
    }
    println!("3. token IDs = {ids:?}");
    let prompt_tokens = u32::try_from(ids.len())?;
    let budget = converted.resolve_max_tokens(prompt_tokens, prompt_tokens + 200)?;
    assert_eq!(budget, 200);
    assert_eq!(converted.inference_options.temperature, Some(0.3));
    assert_eq!(converted.inference_options.top_p, Some(1.0));
    println!("4. 后端应使用的 max_tokens = {budget}（调用方还需把结果传给后端）");
    let mut chunks = vec![InferenceChunk::Ready {
        system_fingerprint: None,
        prompt_usage: PromptUsage {
            prompt_tokens: ids.len(),
            prompt_cache_hit_tokens: 0,
        },
    }];
    chunks.extend(texts(&["Hello ", "world!"]));
    let response = collect_response(converted, chunks, None, true).await?;
    let expected = if stop.is_some() {
        "Hell"
    } else {
        "Hello world!"
    };
    assert_eq!(response["choices"][0]["message"]["content"], expected);
    println!("5. 完整响应 = {response}");
    Ok(())
}

async fn thinking() -> TutorialResult {
    for (label, mode, parts, expected_content, expected_reasoning) in [
        (
            "默认 thinking，未闭合",
            None,
            vec!["Hello ", "world!"],
            "",
            "Hello world!",
        ),
        (
            "显式关闭 thinking",
            Some(false),
            vec!["Hello ", "world!"],
            "Hello world!",
            "",
        ),
        (
            "默认 thinking，跨片段闭合",
            None,
            vec!["先思考", "</th", "ink>Hello world!"],
            "Hello world!",
            "先思考",
        ),
    ] {
        let response = collect_response(request(mode, None)?, texts(&parts), None, false).await?;
        let message = &response["choices"][0]["message"];
        assert_eq!(message["content"].as_str().unwrap_or(""), expected_content);
        assert_eq!(
            message["reasoning_content"].as_str().unwrap_or(""),
            expected_reasoning
        );
        println!("{label}：{message}");
    }
    Ok(())
}

fn boundaries() -> TutorialResult {
    let mut converted = request(Some(false), None)?;
    assert_eq!(converted.resolve_max_tokens(5, 205)?, 200);
    assert_eq!(converted.resolve_max_tokens(5, 5)?, 1);
    assert!(converted.resolve_max_tokens(6, 5).is_err());
    converted.inference_options.max_tokens = Some(300);
    assert!(converted.resolve_max_tokens(5, 205).is_err());
    let messages: MessagesRequest = serde_json::from_value(json!({
        "model": MODEL, "messages": [{"role": "user", "content": "Hello"}]
    }))?;
    assert!(
        messages
            .convert(ConversionOptions::default())?
            .inference_options
            .max_tokens
            .is_none()
    );
    println!(
        "边界通过：预算 200；满窗口返回 1；输入超限/显式要求 300 被拒绝；Messages 可省略上限。"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> TutorialResult {
    match std::env::args().nth(1).as_deref().unwrap_or("basic") {
        "basic" => basic(None).await,
        "stop" => basic(Some("o wor")).await,
        "thinking" => thinking().await,
        "boundaries" => boundaries(),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "场景应为 basic、stop、thinking 或 boundaries",
        )
        .into()),
    }
}

#[cfg(test)]
mod tests;
