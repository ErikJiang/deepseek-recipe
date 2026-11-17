# 03 Token 与上下文预算：模型收到的是数字

Token 是模型处理文本的单位，可能是一个字、词的一部分或一个控制标记。Tokenizer 按模型词表把这些单位变成 token IDs。计算输入长度时要数 ID，不能拿字符数代替。

```text
Conversation → 渲染文本和特殊区间 → 匹配的 tokenizer → token IDs
                                                           ↓
                                            算输入长度，再确定输出预算
```

项目的 [PromptTokenizer](../deepseek-recipe-encoding/src/tokenizer.rs) 对显式特殊标记查特殊词表，对其他内容按普通文本分词。V4.1 模板配 V4.1 的 `tokenizer.json`。调用 `render_conversation` 只有文本；附加 tokenizer 后调用 `encode` 才得到 IDs。

## 先认识几个词

| 词 | 在哪里用 |
| --- | --- |
| Vocabulary（词表） | 给 tokenizer 查询文本单位对应的 ID。项目为 V4 和 V4.1 提供匹配的词表文件。 |
| Context window（上下文窗口） | 模型单次请求能容纳的总 token 数，覆盖输入与生成。 |
| `max_tokens` | 请求允许生成的上限。请求转换时先校验它是否合法，编码后再结合上下文确定它是否放得下。 |
| Usage（用量） | 实际处理的输入和输出 token 数。它可以比设定的生成上限小，例如模型提前停止。 |

设后端上下文上限是 1,000，输入占 800。显式传 `max_tokens: 150` 可以保留，传 300 会报错。省略时由库选默认值，再根据剩余空间限制为 200。计算的是 token IDs 的个数。

## 跟着实际数字计算一次

Rust `basic` 场景在当前内置词表上得到：

```text
prompt: <｜begin▁of▁sentence｜><｜User｜>Hello<｜Assistant｜></think>
IDs:    [0, 128803, 19923, 128804, 128822]
数量:   5
教学上下文上限: 5 + 200 = 205
请求没有 max_tokens → 可用生成预算 200
```

这些编号只用于核对仓库词表，不能拿去套其他模型。这里恰好一个 `Hello` 对应一个 ID，并不意味着每个英文单词都是一个 token。中文、空格、标点和罕见字符都可能改变切分。

| 同一输入长度 5 | 上限 205 下的结果 | 检查发生在哪里 |
| --- | --- | --- |
| 未指定生成上限 | 200 | 默认值受剩余空间限制 |
| 显式指定 150 | 150 | 输入加输出没有超限 |
| 显式指定 300 | 错误 | `5 + 300 > 205` |

在当前源码中，未指定上限时，非思考默认值为 8192，思考通常为 65536，思考且 effort 为 Max 时为 131072。它们是**生成上限的默认值**，不是模型上下文大小。上下文大小由接入方根据目标后端提供。参见 [默认值与预算](../deepseek-recipe/src/request/mod.rs)。

`resolve_max_tokens` 返回一个数值，不会替调用者修改 `inference_options.max_tokens` 或启动推理。正确的交接是：

```text
编码 → 计算输入 token 数（含图片调整）
     → resolve_max_tokens 得到最终上限
     → 将这个返回值填进后端请求
```

这是维护时容易遗漏的一步：日志里算出了 200，并不等于引擎实际收到了 200。

## 跟读 Rust

先在 `PromptEncoding::encode` 找「渲染 → 编码」的交接，再读 `PromptTokenizer::encode_ids`。它按 `special_ranges` 切出普通区间，批量分词后按原顺序插回特殊 ID。下面是组装阶段的源码节选。 [PromptEncoding](../deepseek-recipe-encoding/src/lib.rs) · [PromptTokenizer](../deepseek-recipe-encoding/src/tokenizer.rs)

```rust
for range in special_ranges {
    if let Some(encoding) = encodings.next() {
        ids.extend_from_slice(encoding.get_ids());
    }
    ids.push(self.special_token_id(&text[range.clone()])?);
}
```

`if let Some(...)` 表示迭代器还有一段分词结果时才处理它。每次循环先追加一个普通区间的 IDs，再插入紧接着的特殊 ID。循环之后还会追加最后一段普通文本。`Range<usize>` 在这里表示 UTF-8 字节范围，切片的起止点必须落在字符边界。

再读 `resolve_max_tokens`。它用 `Option<u32>` 表示「显式指定」或「省略」。先检查输入与显式上限之和，再在省略时选默认生成上限，最后按剩余空间限制。实现最低返回 1，所以调用方还要留意输入刚好填满上下文这个边界，并按后端限制处理。 [上下文预算实现](../deepseek-recipe/src/request/mod.rs)

下面是函数的最后几行。前面的超限校验已经完成。这里的 `default` 是按思考模式设置的默认生成上限。`unwrap_or(default)` 在请求省略上限时才取它，`.min(remaining)` 限制剩余空间，随后 `.max(1)` 又将结果抬到至少 1。

```rust
let remaining = context_limit - prompt_tokens;
Ok(self
    .inference_options
    .max_tokens
    .unwrap_or(default)
    .min(remaining)
    .max(1))
```

具体边界是输入恰好等于上下文上限且未指定 `max_tokens` 时，此函数仍返回 1。真实后端接入要在调用前确认还有生成空间。可把 `prompt_tokens >= context_limit` 视为需要拒绝或缩短输入的情况。这个边界由 [Rust 练习](examples/main.rs)的 `boundaries` 场景断言演示：

```sh
CARGO_TARGET_DIR=target cargo run --manifest-path tutorials/examples/Cargo.toml --locked -- boundaries
```

## Python 支线：编码并计算还可生成多少

按 [Python 示例服务器的安装步骤](../server-py/README.md#usage)准备 bindings。在仓库根目录、已激活的环境中执行：

```sh
python3 - <<'PYTHON'
from deepseek_recipe import ChatCompletionRequest, ConversionOptions
from deepseek_recipe import DeepseekV41Encoding, Tokenizer

request = ChatCompletionRequest({
    "model": "DeepSeek-V4.1-Flash",
    "messages": [{"role": "user", "content": "Hello"}],
    "thinking": {"type": "disabled"},
}).convert(ConversionOptions())

tokenizer = Tokenizer.from_file("static/tokenizers/v41/tokenizer.json")
encoding = DeepseekV41Encoding().with_tokenizer(tokenizer)
ids = encoding.encode(request.conversation)
context_limit = len(ids) + 200  # 教学用上限
print(ids)
print(request.resolve_max_tokens(len(ids), context_limit))
PYTHON
```

最后一行应为 `200`：未指定 `max_tokens` 时，默认生成上限被剩余空间限制。如果在请求里显式加 `"max_tokens": 300`，转换后再算预算时会报错。真实接入要使用目标后端支持的上下文上限；图片请求还要[加上图片 token 差额](07-image-input.md#为什么要补算-token)。

参数校验与上下文预算是两道检查：转换时校验请求参数范围；编码后才能用 [resolve_max_tokens](../deepseek-recipe/src/request/mod.rs) 检查输入与输出上限是否放得下。只有 Rust 环境时，可以从仓库根目录运行配套综合练习，它会实际加载内置 tokenizer，验证 200 的预算结果，并把模拟输出解析成协议响应。

```sh
CARGO_TARGET_DIR=target cargo run --manifest-path tutorials/examples/Cargo.toml --locked
```

练习使用本地 crate 与仓库内的 tokenizer，第一次构建可能下载 Cargo 依赖。完整源码及运行结果说明见[课程索引](README.md#可运行的-rust-综合练习)。

## 自测

把一条历史消息加到对话里，即使 `max_tokens` 不变，为什么原先成功的请求仍可能超限？

<details>
<summary>参考答案</summary>

模板会把历史消息和相应标记写入 prompt，编码后的输入 token 数可能增加。上下文预算检查的是输入 token 数加上显式要求的生成上限。应对重新渲染和编码后的完整输入计算预算。

</details>

---

[上一章](02-prompt-rendering.md) · [课程目录](README.md) · [下一章](04-inference-backend.md)
