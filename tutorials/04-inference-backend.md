# 04 推理后端：换掉 mock，保留 API

先沿 Rust 综合练习看清后端交接，再按需做 Python 注入实验。Python 示例通过回调接收推理后端：`create_app(infer=...)`。回调收到 `PreparedRequest`，里面有渲染后的 prompt、推理参数和处理后的图片。它逐段返回 `InferenceChunk`，服务端负责把这些片段变成客户端认识的响应。

| 后端片段 | 含义 |
| --- | --- |
| `Ready` | 生成前的元数据，例如输入 token 用量 |
| `Text` | 一段文本及其对应的生成 token 数 |
| `Tokens` | 一批已生成的 token IDs，可附带 token 概率信息 |
| `Finish` | 结束原因，例如正常停止 `Stop` 或达到生成上限 `Length` |

参见 [InferenceChunk](../deepseek-recipe/src/stream/inference.rs)。示例的 Python `StreamProcessor` 没有附加 tokenizer，所以自定义回调应返回 `Text`。要改用 `Tokens`，还需给 processor 接入匹配的 tokenizer。

## 先认识几个词

| 词 | 在一次生成中起什么作用 |
| --- | --- |
| Inference engine（推理引擎） | 运行模型权重、管理请求并生成 token。示例用 mock 模拟它的输出接口。 |
| Prefill | 处理整个输入，建立生成需要的状态，并产生第一个输出 token 所需的概率。 |
| Decode | 使用已有上下文，反复预测和选择下一个 token，直到结束。 |
| KV cache | 保存已处理 token 的注意力键和值，供后面的生成步骤复用。它会占用内存。 |
| TTFT / TPOT | 从请求发出到收到首 token 的等待时间，以及首 token 之后平均每生成一个 token 的时间。 |

例如输入由 `Hello` 和几个控制 token 组成。真实引擎先处理这些输入，从候选中选出第一个输出 token，再基于已有输入和输出继续生成。一次请求的 TTFT 可能还包含排队、输入准备和网络时间。模拟后端适合验证接口、事件顺序和解析结果。 [DistServe](https://www.usenix.org/conference/osdi24/presentation/zhong-yinmin)

## 跟读 Rust

打开 `prepare_processor<T>`，看它如何根据 `model_version` 选择模板、处理图片，再用 `T::chunk_generator` 建立对应协议的生成器。随后 `complete_response<T>` 用下面的源码从后端读取片段。 [Rust 示例服务器](../server-rs/src/lib.rs)

```rust
let mut chunks = pin!(processor.into_protocol_stream(mock_inference()));
while let Some(chunk) = chunks.next().await {
    let chunk = chunk.map_err(|error| ConversionError::internal(error.to_string()))?;
    response.accumulate(chunk);
}
```

`Stream` 可以想成异步迭代器，每次调用 `next().await` 等待下一项。`pin!` 把流固定在当前内存位置，让它可以安全地跨多次等待保留内部状态。`map_err` 把解析错误改成服务层能处理的错误。`accumulate` 将一个协议增量合入完整响应。

把目光移到 `mock_inference()` 这个参数。它代表后端输出边界。Rust 示例的两条响应路径都调用同一个 mock。下面的 Python 练习通过已有的 `create_app(infer=...)` 注入自己的实现，能单独验证这个接口。 [片段与结束原因](../deepseek-recipe/src/stream/inference.rs)

## Rust 主线：后端只需遵守片段契约

继续运行 `basic` 场景即可观察 Ready、两段 Text、Finish 如何变成协议响应。打开 [main.rs](examples/main.rs)，找到 `chunks.extend(texts(...))` 和 `collect_response`。`iter(chunks)` 把固定数组转换成可消费的流；真实引擎则在计算结果产生后逐步提供片段。

```sh
CARGO_TARGET_DIR=target cargo run --manifest-path tutorials/examples/Cargo.toml --locked -- basic
```

| 交接方向 | 需要确认的数据 | 谁负责 |
| --- | --- | --- |
| 服务 → 引擎 | 模板/IDs、图片、最终预算、采样与约束 | 接入方构造正确引擎请求 |
| 引擎 → processor | 增量 Text 或 Tokens、用量、结束原因 | 适配器映射真实输出 |
| processor → HTTP | 协议 chunk 或处理错误 | 服务选择累积或 SSE |

本例的预算计算和 Ready 用量是真实加载 tokenizer 后得到的；输出文本和 completion token_count 是手工设定的。它能验证交接和解析，不能验证模型是否遵守 max_tokens 或采样参数。

第一次修改只改输出文字和最终断言，确认 API 结果随之变化。之后再做第 10 章的[累计文本适配实验](10-backend-adapter.md)，学习用量、结束和取消处理。`Stream`、关联类型和完整累积函数见[第 09 章](09-rust-practice.md)。

## Python 支线：换成自己的模拟后端

按 [server-py 的 Usage](../server-py/README.md#usage) 安装依赖。若原版服务器占用 7777，先在它的终端按 Ctrl+C 停止。把下面内容保存为 `server-py/tutorial_backend.py`。这个练习文件可以随时删除。

```python
from deepseek_recipe import InferenceChunk, InferenceFinishReason, SpecialToken
from main import create_app

async def infer(request):
    yield InferenceChunk.ready()
    if request.prompt.endswith(SpecialToken.ThinkingStart.default_str()):
        yield InferenceChunk.text(SpecialToken.ThinkingEnd.default_str())
    yield InferenceChunk.text("From tutorial backend.")
    yield InferenceChunk.finish(InferenceFinishReason.Stop)

app = create_app(infer=infer)
```

在仓库根目录运行：

```sh
python3 -m uvicorn tutorial_backend:app --app-dir server-py --host 127.0.0.1 --port 7777
```

在另一个终端发请求，预期正文为 `From tutorial backend.`。

```sh
curl -sS http://127.0.0.1:7777/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"deepseek-flash","messages":[{"role":"user","content":"Hello"}],"thinking":{"type":"disabled"}}'
```

示例里的文本计数未填写，默认是零。真实后端应报告用量；如果客户端断开，回调也要释放正在进行的生成任务。参考 [response_body 与 create_app](../server-py/main.py) 的关闭流程。

## 接入真实模型时，先核对这三件事

1. **输入约定。** 确认引擎要 prompt 文本还是 token IDs。自己编码时，用 `Conversation`、匹配的模板和 tokenizer，保留特殊区间的语义；按[第 03 章](03-token-budget.md)计算上下文预算。避免让引擎对已渲染的 prompt 再套一次对话模板。
2. **生成约定。** 后端负责加载权重、排队和采样。真实生成先处理输入并得到第一个 token（prefill），再逐步生成后续 token（decode）。KV cache 让后续步骤复用已处理 token 的注意力键和值。示例回调不测这些计算；延迟指标见[入门导览](00-overview.md)。
3. **输出约定。** 确认片段类型、结束原因、用量和取消行为。如果上游给的是增量文本，应把新片段交给处理器；如果上游给的是累计文本，应先取新增部分。

## 后端结束后，谁负责收尾？

回调正常结束时，建议明确发送 `Finish`，保留 `Stop`、`Length`、`ContentFilter` 的区别。流直接耗尽时，库会生成内部 `EndOfStream`，Chat Completions 最终会把它映射为 `stop`，所以单看这个字段不能证明后端正常完成。排查截断时，还要记录后端实际结束原因。 [结束原因约定](../deepseek-recipe/src/stream/inference.rs) · [协议映射](../deepseek-recipe/src/protocol/openai/chat_completion/response/schema.rs)

客户端断开或处理器命中停止词时，Python 示例在 `finally` 中关闭推理迭代器和 processor。真实后端应把生成任务的取消与释放写在自己迭代器的清理逻辑里，异步清理还需要正确处理取消。只停止发送 HTTP 字节，可能让上游仍在计算。 [response_body 和 InferenceStreamingResponse](../server-py/main.py)

接入 token IDs 时还要核对仓库的终止约定。`InferenceFinishReason` 的注释规定，后端最终输出携带的 token IDs 只在 `Length` 时继续转发，其余结束原因丢弃该终止输出中的 IDs。映射具体引擎时，应先区分正常增量与终止元数据，再按这个约定适配。

本章的自定义 mock 面向普通文本问答。工具调用和 prefix 续写要按各自的初始解析状态实现，可结合第 02、06 章继续扩展。

## 自测

外部引擎先发 `Hel`，再发 `Hello`，并说明每次发送的是累计文本。应交给 `InferenceChunk::Text` 哪两段？

<details>
<summary>参考答案</summary>

传 `Hel` 和 `lo`。处理器按顺序消费增量。传 `Hel` 和整个 `Hello` 会让正文重复。真实接入还应从引擎获取对应的 token 用量，不能拿字符串长度充当 token 数。

</details>

---

[上一章](03-token-budget.md) · [课程目录](README.md) · [下一章](05-streaming-response.md)
