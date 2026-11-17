# 08 从现象到 PR：复现、定位、验证

分析 issue 时，从第一个出现差异的地方入手。请求收到了吗？转换结果对吗？Prompt 正确吗？后端返回了什么？客户端实际收到了什么？每次只改变一个输入条件，直到留下能触发问题的最小请求。

| 现象 | 优先检查 | 需要留的证据 |
| --- | --- | --- |
| 请求返回 400 | 协议转换、模型别名、参数校验 | API 路径、请求 JSON、错误响应 |
| 角色或思考标记不对 | V4/V4.1 模板、转换后的消息 | 模型名、Demo 的实际 prompt |
| 输入太长或输出过早停止 | Tokenizer、上下文预算、后端结束原因 | 输入 token 数、上下文上限、实际结束原因 |
| 正文混入标记，或停止词没生效 | `OutputParser`、`StreamProcessor` | 原始片段及切分位置、期望正文 |
| 只有流式或某个 API 格式异常 | 对应协议的响应生成器、SSE 封装 | 完整 JSON 与 SSE 对照、协议事件名 |

报告用仓库版本、最小请求、实际结果和预期结果。用合成文本替代私有内容，移除凭据。上游开发约定和依赖见[开发指南](../docs/development.md)。

## 先认识几个词

| 词 | 在一次修复中起什么作用 |
| --- | --- |
| Issue | 一份问题或需求记录。对于 bug，至少要让别人知道在哪个版本、怎么触发、实际和预期各是什么。 |
| 最小复现 | 能稳定触发问题的最小输入和步骤。移除一个字段后仍能复现，就可以考虑删掉它。 |
| 回归测试 | 用失败和成功都能观察到的断言固定预期，防止以后改坏同一行为。 |
| PR（Pull Request） | 申请合入的改动。维护者需要能看懂影响范围、修改理由和验证结果。 |
| Crate | Rust 的编译单位。这个项目的 workspace 里有多组代码，测试命令可用 `-p` 选择对应包。 |

例如收到「流式正文少了字符」的报告。先拿固定的后端片段分别请求 JSON 和 SSE。如果两边都少字，就进一步跳过 HTTP，直接把片段交给 `StreamProcessor`。如果只有 SSE 少字，再对比生成器事件、SSE 分帧和客户端拼接逻辑。这一步能帮你选对测试层。

## 跟读 Rust

下面是跨片段停止词测试的源码节选。读它时先找输入、动作、预期三部分。 [processor.rs 中的回归测试](../deepseek-recipe/src/stream/processor.rs)

```rust
let options = ParsingOptions {
    stop_sequences: vec!["o worl".to_string()],
    ..ParsingOptions::default()
};
// The stop sequence completes in `ld`; `!` is not read.
let chunks = parse_texts(&["he", "llo", " ", "wor", "ld", "!"], options).await;
assert_eq!(content_chunks(&chunks), ["he", "ll"]);
let (finish_reason, stop_sequence, completion_usage) = finish_chunk(&chunks);
assert_eq!(finish_reason, FinishReason::StopSequence);
assert_eq!(stop_sequence, Some("o worl"));
assert_eq!(completion_usage.completion_tokens, 5);
```

`..ParsingOptions::default()` 表示其他字段沿用默认值。`parse_texts` 在测试内向处理器传入分段文本。`assert_eq!` 将实际和预期对照，值不一致时测试失败。这里不仅核对正文，还核对结束原因和用量。第 5 段 `ld` 使停止词匹配成功，测试辅助函数给每段计一个 token，第 6 段未消费。读相邻的 `a_failed_stop_sequence_prefix_match_backtracks`，对比候选匹配失败后怎样恢复正文。

## 完整案例：为什么 Hello 跑到了 reasoning_content？

这个案例来自教程审阅中的实际偏差：旧导览省略 thinking，却承诺 Rust mock 的正文为 Hello world!。目标是修正文档实验与验证，让正常库行为得到准确呈现。

### 第一步：写下可检查的报告

```text
版本：recipe 935817c
入口：Chat Completions
输入：user Hello，未指定 thinking
上游固定片段：Text("Hello ")、Text("world!")、Finish(Stop)
实际：content 为空，reasoning_content 为 Hello world!
旧教程预期：content 为 Hello world!
影响：初学者可能把正确的解析结果误判为安装或调用失败
```

### 第二步：跳过 HTTP，重现差异

[tests.rs](examples/tests.rs)中的 `tutorial_content_contract` 默认验证修正后的实验。下面的环境变量只让这一教学测试采用旧请求，不修改核心库或服务器。预期命令失败，并显示空正文与 Hello world! 的差异：

```sh
CARGO_TARGET_DIR=target TUTORIAL_REPRODUCE_BUG=1 cargo test --manifest-path tutorials/examples/Cargo.toml --locked tutorial_content_contract -- --nocapture
```

失败是本步骤的验收结果。如果看见依赖下载失败或编译错误，则还没重现到目标问题，不能把它当作同一次失败。

### 第三步：按证据排除假设

| 假设 | 查看什么 | 本例结论 |
| --- | --- | --- |
| 后端根本没返回文字 | 固定的 Text 片段 | 有完整 Hello world! |
| SSE 把字符丢了 | 绕开 HTTP 后仍复现 | 不是 SSE 分帧造成的 |
| 字符被解析器丢弃 | 完整响应的 reasoning_content | 文字保留在另一个字段 |
| 初始状态与预期不一致 | 转换后的 thinking_initial_stage | 省略 thinking 后从 Reasoning 开始 |
| mock 会自动结束思考 | Rust mock 的全部输出 | 没有结束思考标记 |

这条推理把问题定位到“实验输入与预期不一致”。核心解析器按请求工作，不能为了让教程通过就把所有 reasoning 强制改成 content。

### 第四步：做最小修正

普通正文入门实验明确关闭 thinking，关键请求差异如下。这里的 diff 展示教学输入变化，不是修改库默认值：

```diff
- {"model":"deepseek-flash","messages":[{"role":"user","content":"Hello"}]}
+ {"model":"deepseek-flash","messages":[{"role":"user","content":"Hello"}],"thinking":{"type":"disabled"}}
```

若实验目的就是演示 thinking，则应让固定输出包含正确的结束思考标记，并分别断言两个字段。不要用“只看 content 非空”代替确认模式切换。

### 第五步：运行修正后的契约与邻近回归

不设置复现变量，测试使用显式关闭 thinking 的请求：

```sh
CARGO_TARGET_DIR=target cargo test --manifest-path tutorials/examples/Cargo.toml --locked tutorial_content_contract
CARGO_TARGET_DIR=target cargo test --manifest-path tutorials/examples/Cargo.toml --locked
```

第一条应通过；第二条还检查默认 thinking 仍产生 reasoning、结束标记可跨片段、停止词范围与 token 来源区别。这样“修正教程”不会被误解成“更改默认模式”。环境变量写在命令前，只作用于那一条命令，无需修改 shell 全局配置。

### 第六步：写出可审查的 PR 描述

```text
修正入门教程的 Rust mock 请求。旧请求默认进入思考阶段，却把预期结果写成正文。
示例现在显式关闭 thinking，并增加默认模式与跨片段结束标记的对照实验。

验证：复现开关下 content 契约失败；修正输入后通过；教学回归测试通过。
范围：请求转换和输出处理；未用此测试证明 HTTP 服务或真实模型已验证。
```

这个案例不需要先理解整个仓库。关键是每一步都用数据缩小范围，而不是从第一个看到的函数开始盲目修改。

## 练习一：把报告缩成一个回归测试

假设有人报告：停止词 `o worl` 分成几段时泄漏进正文。在 [processor.rs](../deepseek-recipe/src/stream/processor.rs) 找到现有测试 `a_stop_sequence_spanning_chunks_ends_output`。它模拟 `["he", "llo", " ", "wor", "ld", "!"]`，要求客户端正文只有 `hell`。该测试已覆盖这个行为；它是学习样例。

```sh
cargo test -p deepseek-recipe --locked a_stop_sequence_spanning_chunks_ends_output
```

如果原报告仍能复现，比较协议、后端片段类型、停止词、片段边界和最终响应。找出与现有用例不同的那一个条件。写一个在修复前失败、修复后通过的回归用例；PR 里注明输入与预期，方便维护者复查。只在目标层做必要改动。

练手时，在原文件的 `tests` 模块里复制这个测试，改名为 `a_stop_sequence_with_merged_prefix`。把前两段 `"he", "llo"` 合成 `"hello"`，再按新名称运行测试。逻辑正文仍应是 `hell`，但正文片段断言应改为 `["hell"]`，消费的模拟 token 数应改为 4。这个差异来自辅助函数每段计一个 token 的设定。先预测结果，再改断言验证；真实引擎的 token 用量应按其实际编码计数。

## 练习二：完成一次不改核心库的二次开发

按[第 04 章](04-inference-backend.md)接入自定义 mock，为它注册一个新的模型别名。参考 [test_custom_inference](../server-py/tests/test_smoke.py) 的写法执行下列检查：

1. 让别名映射到 V4.1。用 Chat Completions 请求该别名，检查回调收到了渲染后的 prompt 和配置的推理参数。
2. 分别用 `stream: false` 与 `stream: true` 请求同一句话。验证完整正文和拼接后的增量正文一致。
3. 换成未注册的模型名，验证返回 400，且回调没有收到请求。想换真实后端时，先按[第 03 章](03-token-budget.md)核对上下文预算。

## 变更落点：改一个功能，哪些层要一起检查？

| 需求 | 首个落点 | 必须确认的下游 |
| --- | --- | --- |
| 新的模型别名 | ConversionOptions 或应用配置 | 模板版本正确；未知名仍报错；权重映射由后端负责 |
| 新增协议字段 | 对应 schema 和 convert | 内部类型是否能表达；默认值、冲突和错误路径；响应是否需要回显 |
| 修改模板 | 对应版本 encoding | special_ranges、历史消息、工具与 prefix；另一版本不被误改 |
| 修复解析边界 | OutputParser / decoder / processor | 多种分片、结束原因、用量；JSON 与 SSE 语义一致 |
| 新增 Python 能力 | Rust API 与 bindings | native 模块重建、pyi 声明、Python 测试与文档 |

以“新增协议字段”为例，先判断现有内部类型能否表达它，再决定是否扩展类型。仅在 schema 增加字段但 convert 不读取，可能形成“能提交、却不生效”的接口。新增测试应直接检查该字段的实际下游作用。

## 独立验收：不看答案完成一次小任务

任选一个任务，并交付最小输入、预期、测试与简短解释：

- 对普通文本停止词，在所有合法字符边界切成两段，验证最终正文不随切分改变。先只固定正文语义；模拟每段计一个 token 的用量可能变化。
- 给新模型别名编写成功与未知别名两条测试，解释别名、模板和实际权重之间的关系。
- 在第 10 章适配器中增加增量上游样本，验证它不会被错误地重复切片。

能运行示例是第一步；能独立选择正确测试层并解释为何修复有效，才达到本章的开发维护目标。对复杂工具、多模态或真实引擎问题，仍需完成对应章节和环境验证。

## 提交前怎么验证

先运行复现问题的最小测试，再按改动范围扩大：协议、模板和 Demo 可跑下方不依赖 OpenCV 的测试；改到 Python bindings、HTTP 示例或图片处理时，按[开发指南](../docs/development.md)准备对应依赖，再运行相关测试与仓库要求的检查。

```sh
cargo test -p deepseek-recipe-core -p deepseek-recipe -p deepseek-recipe-encoding -p encoding-decoding-demo --locked
```

PR 用一两句话写清「什么输入触发了什么问题、改完后会怎样」，附上验证命令和结果。遇到只在某个后端才能复现的差异，记录这个限制，方便维护者判断还要验证哪里。可回到[课程索引](README.md)查对应环节。

## Rust 改动如何到达 Python？

Python 包通过 PyO3 bindings 调用 Rust。Rust API 有新增或修改时，沿着「Rust 实现 → bindings → 类型声明 → Python 测试」检查。只重跑旧的 Python 环境，可能仍加载上一次编译的 native 模块。

| 改动 | 同时检查 |
| --- | --- |
| 对话或推理选项 | [request.rs bindings](../deepseek-recipe-python/src/request.rs) 与 [类型声明](../deepseek-recipe-python/python/deepseek_recipe/_native.pyi) |
| 渲染或 tokenizer | [encoding.rs bindings](../deepseek-recipe-python/src/encoding.rs) 和 [bindings 测试](../deepseek-recipe-python/tests/test_bindings.py) |
| 流式解析或关闭行为 | [response.rs bindings](../deepseek-recipe-python/src/response.rs) 和 [服务器测试](../server-py/tests/test_smoke.py) |

按开发指南重新执行 `maturin develop --manifest-path deepseek-recipe-python/Cargo.toml --locked`，再跑 Python 测试。还应同步受影响的中英文 API 文档。工程检查命令及依赖以[开发指南](../docs/development.md)为准。

## 自测

同样的后端片段在 JSON 和 SSE 都漏字。你会优先加 HTTP 测试，还是直接在解析器的测试里复现？PR 描述至少要让维护者看到什么？

<details>
<summary>参考答案</summary>

先尝试在解析器中复现，以固定输入片段排除传输差异。如果解析行为正确，再往协议生成器和累积逻辑查。PR 至少应说明复现输入、观察到的差异、修复后的预期以及运行过的验证命令。

</details>

---

[上一章](07-image-input.md) · [课程目录](README.md) · [下一章](09-rust-practice.md)
