# 01 请求转换：不同 API 先说同一种话

同一句 `Hello` 可以从不同 API 入口进入。转换层读取各自的 JSON，检查字段，再产出共同的 `ConversationRequest`。后面的模型模板可以读同一种数据。

| API 入口 | 表达一条用户消息的核心字段 |
| --- | --- |
| `POST /v1/chat/completions` | `"messages":[{"role":"user","content":"Hello"}]` |
| `POST /v1/responses` | `"input":"Hello"` |
| `POST /v1/messages` | `"messages":[{"role":"user","content":"Hello"}]` |

表里只列消息字段。实际请求还要提供 `model`。本项目的 Messages 请求把 `max_tokens` 定义为 `Option<u32>`，允许省略，编码后再解析默认生成预算。三种协议共享对话结构，但各自保留不同的字段规则、默认值和返回格式。这里介绍的是当前仓库的行为。 [Messages 请求类型](../deepseek-recipe/src/protocol/anthropic/messages/request/schema.rs)

## 先认识几个词

| 词 | 在这个项目里是什么意思 |
| --- | --- |
| API 协议 | 客户端和服务端约定的请求字段、响应字段和错误格式。一个协议通常有自己的 endpoint，也就是 HTTP 路径。 |
| 反序列化 | 把收到的 JSON 字节读成 Rust 类型。例如把 Chat Completions 的 JSON 读成 `ChatCompletionRequest`。 |
| 协议转换 | 把各 API 的字段校验并整理成内部数据。反序列化成功后，仍可能因为模型名或参数不受支持而转换失败。 |
| 采样参数 | 告诉后端怎样从候选 token 中选下一个。`temperature` 调节候选分布，`top_p` 按累计概率选取候选范围。 |

例如 Chat Completions 的 `messages` 和 Responses 的 `input` 都能表达「用户说 Hello」。转换器保留这个共同含义，同时记录输出解析和传输需要的选项。它们此时仍是结构化数据，下一章才会把它们渲染成 prompt。

```text
HTTP 请求体
  → 按 endpoint 选择请求类型
  → serde_json 按类型读 JSON
  → convert 检查语义并解析模型版本
  → ConversationRequest
```

## 转换之后，谁读什么？

| 字段 | 下游用途 |
| --- | --- |
| `conversation`、`model_version` | 记录消息并选出 V4 或 V4.1 模板 |
| `inference_options` | 告诉后端用什么采样参数、输出上限或约束 |
| `parsing_options` | 告诉输出解析器是否处理思考、工具标记和停止词 |
| `stream`、`response_options` | 决定如何返回响应，例如是否附带流式用量 |

读 [Chat Completions 的 convert](../deepseek-recipe/src/protocol/openai/chat_completion/request/convert.rs)、[Responses 的 convert](../deepseek-recipe/src/protocol/openai/responses/request/convert.rs) 或 [Messages 的 convert](../deepseek-recipe/src/protocol/anthropic/messages/request/convert.rs)。先看 `convert` 怎么构建 [ConversationRequest](../deepseek-recipe/src/request/mod.rs)，再追某个字段的校验。服务注册的模型别名可在 [conversion_options](../server-py/main.py) 找到。

## 动手主线：观察转换前后的同一条请求

本章起，Rust 主线统一使用库默认模型名 `DeepSeek-V4.1-Flash`。浏览器 Demo 与 HTTP 示例使用已注册的别名 `deepseek-flash`。第一次练习显式关闭 thinking，后面再增加复杂性。

```sh
CARGO_TARGET_DIR=target cargo run --manifest-path tutorials/examples/Cargo.toml --locked -- basic
```

无需 OpenCV、Python bindings 或 GPU。关注输出中的 `conversation`、`inference_options` 和 `parsing_options`。对应输入如下：

```json
{
  "model": "DeepSeek-V4.1-Flash",
  "messages": [{"role": "user", "content": "Hello"}],
  "thinking": {"type": "disabled"},
  "temperature": 0.3,
  "top_p": 0.8,
  "stream": true
}
```

下面是关键字段摘录，省略了与本例无关的字段；它不是完整 Rust 结构或另一份请求 JSON。

| 原始输入 | 转换后 | 为什么这样处理 |
| --- | --- | --- |
| `model` 字符串 | `model_version = DeepseekV41Flash` | 别名查表后，用枚举选择模板 |
| 一条 user 消息 | `conversation.messages` 中的一条 User | 把协议字段转成通用消息结构 |
| `thinking.type = disabled` | `thinking_mode = false`，`thinking_initial_stage = None` | 渲染器和解析器共享同一模式决定 |
| `temperature = 0.3` | `Some(0.3)` | 非思考模式保留该合法值 |
| `top_p = 0.8` | `Some(1.0)` | 当前 V4.1 非思考规则覆盖了请求值 |
| 未给 `max_tokens` | `None` | 此时尚不知道完整 prompt 的 token 数 |
| 未给 `stop` | 空的 `stop_sequences` | 没有客户端指定的停止词 |
| `stream = true` | `stream = true` | 留给响应生成与 HTTP 层使用 |

这里最重要的观察是：**请求字段不一定原样到达后端。** 排查“采样参数不生效”，应分别记录输入值、转换值和引擎实际收到的值。只看 curl 请求无法确定问题发生在哪里。

### 为什么需要统一的内部结构？

假设每种 API 都单独实现模板、停止词和工具解析，修改一个行为就要改三套代码。当前设计把协议差异集中在两端，中间共用 `Conversation` 与输出处理器。代价是转换层必须明确处理不支持的字段，不能声称三个 API 完全等价。

| 同一语义 | Chat Completions | Responses | Messages |
| --- | --- | --- | --- |
| 用户说 Hello | `messages` 中的 user | `input` 字符串或输入项 | `messages` 中的 user |
| 完整正文位置 | `choices[].message.content` | `output` 的 message 项中 `output_text.text` | `content` 的 text 块中 `text` |
| 工具结果关联 | `tool_call_id` | `function_call_output.call_id` | `tool_result.tool_use_id` |

读源码时先选 Chat Completions 跟到底，再用相同语义对照另两种协议。有关工具字段的完整请求见第 06 章。

## 跟读 Rust

从 Rust 服务器的 `main` 看路由。Chat Completions 路径绑定 `api_handler::<ChatCompletionRequest>`。`T` 是泛型参数，这次具体为 `ChatCompletionRequest`。Axum 的 `Json<T>` 先把请求体读成这个类型，随后进入以下源码。 [server-rs/src/main.rs](../server-rs/src/main.rs)

```rust
let request = req.convert(conversion_options()).map_err(http_error)?;
if !request.stream {
    let response = complete_response::<T>(request).await.map_err(http_error)?;
    return Ok(Json(response).into_response());
}
```

`?` 表示成功就取出值，失败就立即返回错误。`map_err(http_error)` 把转换错误映射为 HTTP 状态码与错误体。上述分支返回完整 JSON，后面的分支调用 `prepare_converted_response` 返回 SSE。畸形 JSON 在 `Json<T>` 提取阶段就可能被框架拒绝，错误码和错误体要以实际 HTTP 入口为准。

`ProtocolRequest` 是 trait，三种请求各自实现 `convert`。其中的 `type Response` 让 Rust 知道输入类型应配哪个响应类型。库中还提供 `handle_request` 和 `prepare_response` 等辅助入口，阅读时要先确认调用方。继续读 Chat Completions 的实现，跟住三个值。 [ProtocolRequest](../deepseek-recipe/src/request/mod.rs) · [Chat Completions 转换](../deepseek-recipe/src/protocol/openai/chat_completion/request/convert.rs)

1. `self.model` 经过 `resolve_model_version`，得到决定模板版本的 `model_version`。
2. `self.messages` 经过 `transform_messages`，整理成内部 `InputMessage`，最后放进 `conversation`。
3. `self.stop` 写入 `parsing_options.stop_sequences`。`self.stream` 写入 `stream`。两个字段留给后续不同环节使用。

可以先在 `convert` 里定位下面这一行。它把 API 收到的模型名转换成库内的 `ModelVersion` 枚举。这里失败时，后面的消息转换也不会执行。 [Chat Completions 转换](../deepseek-recipe/src/protocol/openai/chat_completion/request/convert.rs)

```rust
let model_version = options.resolve_model_version(&self.model)?;
```

## 练习：定位一个 400

先记住两个默认行为。库默认接受 `DeepSeek-V4-Pro` 和 `DeepSeek-V4.1-Flash`，Demo 与示例服务器另外注册 `deepseek-v4-pro`、`deepseek-flash`。因此把 Demo 的请求直接交给默认库配置时，要先确认别名已注册。

另外，默认开启思考模式。显式 `thinking` 优先决定开关，否则由 reasoning effort 决定，再回退到默认值。转换后的采样参数也会调整，例如思考模式的 `temperature` 为 1.0，V4.1 非思考模式的 `top_p` 为 1.0。调试时查看最终 `InferenceOptions`，再核对后端收到的值。 [模式解析](../deepseek-recipe/src/protocol/reasoning.rs) · [采样校验](../deepseek-recipe/src/protocol/validation.rs) · [模型选项](../deepseek-recipe/src/request/options.rs)

按[入门导览](00-overview.md)启动 Demo，在 **Render → Chat Completions** 粘贴：

```json
{
  "model": "deepseek-flash",
  "messages": [{"role": "user", "content": "Hello"}],
  "thinking": {"type": "disabled"},
  "max_tokens": 32
}
```

先验证能够渲染，再只改 `model` 为 `unknown-demo-model`。请求应返回 400。原因是 [resolve_model_version](../deepseek-recipe/src/request/options.rs) 查不到模型名，转换在渲染或调用后端之前就停止了。

可以进一步在自己的应用中用 `ConversionOptions.with_model_alias` 注册别名。别名决定库选择哪个模板，实际用哪个模型权重仍由后端配置决定。

## 自测

JSON 能反序列化，但 `model` 未注册。你会追 `serde_json::from_slice` 还是 `resolve_model_version`？如果只改 `stream`，你首先检查哪一个内部字段？

<details>
<summary>参考答案</summary>

未注册的模型名在 `resolve_model_version` 查表时失败。只改 `stream` 时，先看转换结果的 `ConversationRequest.stream`，再沿 HTTP 响应分支走。请求体的 JSON 语法在这两个情形里都可以是正确的。

</details>

---

[上一章](00-overview.md) · [课程目录](README.md) · [下一章](02-prompt-rendering.md)
