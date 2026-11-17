//! 教材中的行为契约：只验证可观察结果，不复制解析器实现。
use super::*;

// 第 08 章：显式开关只用于重现旧教程的错误输入，常规测试走修正后的输入。
#[tokio::test]
async fn tutorial_content_contract() -> TutorialResult {
    let mode = if std::env::var_os("TUTORIAL_REPRODUCE_BUG").is_some() {
        None
    } else {
        Some(false)
    };
    let response = collect_response(
        request(mode, None)?,
        texts(&["Hello ", "world!"]),
        None,
        false,
    )
    .await?;
    assert_eq!(response["choices"][0]["message"]["content"], "Hello world!");
    Ok(())
}

#[tokio::test]
async fn documented_scenarios() -> TutorialResult {
    basic(None).await?;
    basic(Some("o wor")).await?;
    thinking().await?;
    boundaries()
}

#[tokio::test]
async fn stop_matches_across_chunks_but_not_in_reasoning() -> TutorialResult {
    let response = collect_response(
        request(Some(true), Some("END"))?,
        texts(&["END", "</th", "ink>Hello E", "ND ignored"]),
        None,
        false,
    )
    .await?;
    assert_eq!(
        response["choices"][0]["message"]["reasoning_content"],
        "END"
    );
    assert_eq!(response["choices"][0]["message"]["content"], "Hello ");
    Ok(())
}

#[tokio::test]
async fn failed_and_unfinished_stop_prefixes_are_released() -> TutorialResult {
    for (parts, expected) in [(vec!["E", "cho"], "Echo"), (vec!["Hello E"], "Hello E")] {
        let response = collect_response(
            request(Some(false), Some("END"))?,
            texts(&parts),
            None,
            false,
        )
        .await?;
        assert_eq!(response["choices"][0]["message"]["content"], expected);
    }
    Ok(())
}

#[tokio::test]
async fn text_and_token_origins_change_marker_meaning() -> TutorialResult {
    let tokenizer = tokenizer()?;
    let mut plain = tokenizer.clone();
    plain.with_added_vocabulary(tokenizers::AddedVocabulary::new());
    let plain_ids = plain
        .encode("think</think> answer", false)?
        .get_ids()
        .to_vec();
    let end = tokenizer
        .token_to_id("</think>")
        .expect("内置词表包含结束思考 ID");
    assert!(!plain_ids.contains(&end));
    let mut special_ids = plain.encode("think", false)?.get_ids().to_vec();
    special_ids.push(end);
    special_ids.extend_from_slice(plain.encode(" answer", false)?.get_ids());
    for (ids, content, reasoning) in [
        (plain_ids, "", "think</think> answer"),
        (special_ids, " answer", "think"),
    ] {
        let chunks = vec![
            InferenceChunk::Tokens {
                token_ids: ids,
                logprobs: None,
            },
            InferenceChunk::Finish {
                finish_reason: InferenceFinishReason::Stop,
            },
        ];
        let response = collect_response(
            request(Some(true), None)?,
            chunks,
            Some(tokenizer.clone()),
            false,
        )
        .await?;
        assert_eq!(response["choices"][0]["message"]["content"], content);
        assert_eq!(
            response["choices"][0]["message"]["reasoning_content"],
            reasoning
        );
    }
    let response = collect_response(
        request(Some(true), None)?,
        texts(&["think</think> answer"]),
        None,
        false,
    )
    .await?;
    assert_eq!(response["choices"][0]["message"]["content"], " answer");
    Ok(())
}

#[tokio::test]
async fn utf8_tokens_wait_for_a_complete_character() -> TutorialResult {
    let tokenizer = tokenizer()?;
    let ids = tokenizer.encode("🤭", false)?.get_ids().to_vec();
    assert!(ids.len() > 1);
    for (count, expected) in [(1, ""), (ids.len(), "🤭")] {
        let mut chunks: Vec<_> = ids[..count]
            .iter()
            .map(|&id| InferenceChunk::Tokens {
                token_ids: vec![id],
                logprobs: None,
            })
            .collect();
        chunks.push(InferenceChunk::Finish {
            finish_reason: InferenceFinishReason::Stop,
        });
        let response = collect_response(
            request(Some(false), None)?,
            chunks,
            Some(tokenizer.clone()),
            false,
        )
        .await?;
        assert_eq!(response["choices"][0]["message"]["content"], expected);
        let expected_tokens = if count == 1 { 0 } else { count };
        assert_eq!(response["usage"]["completion_tokens"], expected_tokens);
    }
    Ok(())
}

#[tokio::test]
async fn token_chunks_require_a_tokenizer() -> TutorialResult {
    let error = collect_response(
        request(Some(false), None)?,
        vec![InferenceChunk::Tokens {
            token_ids: vec![1],
            logprobs: None,
        }],
        None,
        false,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<deepseek_recipe::stream::StreamError>(),
        Some(deepseek_recipe::stream::StreamError::MissingTokenizer)
    ));
    Ok(())
}

#[tokio::test]
async fn tool_round_trip_preserves_arguments() -> TutorialResult {
    let tools = json!([{"type":"function","function":{
        "name":"get_weather","description":"查询天气",
        "parameters":{"type":"object","properties":{"location":{"type":"string"}},"required":["location"]}
    }}]);
    let body = json!({"model":MODEL,"thinking":{"type":"disabled"},
        "messages":[{"role":"user","content":"北京天气？"}],"tools":tools});
    let input: ChatCompletionRequest = serde_json::from_value(body)?;
    let response = collect_response(
        input.convert(ConversionOptions::default())?,
        texts(&[
            "<｜DSML｜tool_calls><｜DSML｜invoke name=\"get_",
            "weather\"><｜DSML｜parameter name=\"location\" string=\"true\">Bei",
            "jing</｜DSML｜parameter></｜DSML｜invoke></｜DSML｜tool_calls>",
        ]),
        None,
        false,
    )
    .await?;
    let message = &response["choices"][0]["message"];
    let call = &message["tool_calls"][0];
    assert_eq!(call["function"]["name"], "get_weather");
    let arguments: Value = serde_json::from_str(call["function"]["arguments"].as_str().unwrap())?;
    assert_eq!(arguments, json!({"location":"Beijing"}));
    assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
    let next: ChatCompletionRequest = serde_json::from_value(json!({
        "model":MODEL,"thinking":{"type":"disabled"},"tools":tools,
        "messages":[{"role":"user","content":"北京天气？"},message,
            {"role":"tool","tool_call_id":call["id"],"content":"20 C"}]
    }))?;
    let converted = next.convert(ConversionOptions::default())?;
    let prompt = DeepseekV41Encoding::new()
        .render_conversation(&converted.conversation)
        .prompt;
    assert!(prompt.contains("get_weather"));
    assert!(prompt.contains("20 C"));
    Ok(())
}

#[tokio::test]
async fn json_output_parsing_does_not_validate_the_schema() -> TutorialResult {
    let input: ChatCompletionRequest = serde_json::from_value(json!({
        "model":MODEL,"thinking":{"type":"disabled"},
        "messages":[{"role":"user","content":"问候"}],
        "response_format":{"type":"json_schema","json_schema":{
            "name":"greeting","strict":true,"schema":{
                "type":"object","properties":{"greeting":{"type":"string"}},
                "required":["greeting"],"additionalProperties":false
            }
        }}
    }))?;
    let converted = input.convert(ConversionOptions::default())?;
    assert!(converted.inference_options.output_constraint.is_some());
    let response = collect_response(converted, texts(&["{\"greeting\":123}"]), None, false).await?;
    assert_eq!(
        response["choices"][0]["message"]["content"],
        "{\"greeting\":123}"
    );
    Ok(())
}
