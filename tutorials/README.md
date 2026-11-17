# deepseek-recipe 推理服务实战教程

你将追踪一条 `Hello` 请求，看到它如何从 JSON 变成模型输入，再变回 API 响应。本系列的目标是让你能解释各层职责、复现问题并实现小型适配。通过章节练习检验能力；真实引擎、复杂多模态与生产维护还需对应环境和更多实践。

本教程按仓库 `935817c`（workspace 版本 `0.2.0`）核对。所述默认值、模型名和协议兼容范围以这份源码为准。适合读过基本程序、能使用终端但刚接触 LLM 推理服务的学习者。

建议先读[入门导览](00-overview.md)，再按 01–05 的请求路径学习。第 09 章提供 Rust 源码主线；06、07 是功能专题；08、10 用来练习维护与接入；11 补充推理原理。没有 LLM 基础时，可先读第 11 章的前四节。

```text
请求 JSON → 01 协议转换 → 02 Prompt → 03 Token 与预算
                                           ↓
客户端响应 ← 05 输出解析与传输 ← 04 推理后端

06 工具与结构化输出：贯穿转换、渲染、生成和解析
07 图片输入：在渲染后预处理，并补算输入 token
08 贡献：复现失败 → 定位 → 修正 → 回归
09 Rust：所有权、类型关系与异步流
10 适配实战：累计上游 → 增量事件 → 清理
11 原理：生成、采样、KV cache 与性能
```

| 章节 | 学完能做什么 | 练习结果 |
| --- | --- | --- |
| [01 请求转换](01-request-conversion.md) | 看懂三种 API 请求怎样汇合 | 解释为什么未注册的模型名返回 400 |
| [02 Prompt 模板](02-prompt-rendering.md) | 看出模板如何交代角色和思考状态 | 在 Demo 中找到角色标记，并对比思考开关 |
| [03 Token 和上下文预算](03-token-budget.md) | 分清 prompt 字符、token IDs 和生成上限 | 编码一次对话，判断输出预算是否超限 |
| [04 推理后端](04-inference-backend.md) | 看懂项目与模型引擎的交接 | 注入自定义 mock，验证客户端收到的新内容 |
| [05 输出解析和 SSE](05-streaming-response.md) | 分清后端片段、协议事件和 HTTP 响应 | 解释跨片段的停止词，并核对 JSON 和 SSE |
| [06 工具调用与结构化输出](06-tools-and-structured-output.md) | 看懂工具往返和输出约束 | 追踪一次工具调用，并说出谁保证 JSON Schema |
| [07 图片输入](07-image-input.md) | 追踪占位符、预处理和图片 token | 用同一请求对比 V4 与 V4.1 |
| [08 Issue、贡献与二次开发](08-contributing.md) | 按证据定位并最小修正 | 亲自运行一个失败与通过的教学契约 |
| [09 Rust 工程实践](09-rust-practice.md) | 解释所有权、关联类型和异步流 | 跟读完整函数，解释一次编译错误 |
| [10 后端适配实战](10-backend-adapter.md) | 映射累计文本、用量与结束原因 | 验证异常、提前关闭和取消 |
| [11 推理原理与性能](11-inference-principles.md) | 解释生成、采样、缓存和延迟 | 手算分布与时间指标，区分测量范围 |

## 怎么读每一章

每章按三个层次学习，不必一次读完所有边界：

1. **观察：** 先读术语与例子，运行成功路径，记下输入、输出和负责的模块。
2. **解释：** 对照数据表和源码，说明为什么转换、谁保存状态、错误从哪里返回。
3. **独立修改：** 先预测变体结果，再改一个条件，最后用测试验证。带“进阶”或“支线”的部分可稍后完成。

主要动手路径使用同一份 Rust 练习。Python 支线明确标出依赖，浏览器 Demo 用于直观看 prompt 与手写模型输出。第 01–03 章沿用同一个 Hello 请求，避免每章重新适应一套输入。

第一次读 Rust，可以把 `struct` 看成一组有名字的字段，把 `enum` 看成几种可能形态，把 `trait` 看成一组类型共同遵守的接口。遇到 `Result<T, E>` 时，先问成功得到什么、失败返回什么。各章会在这些写法第一次影响流程时解释它们。

教材中的 Rust 代码片段会标明「源码节选」或「简化示意」。节选适合与所在函数对照阅读。章末带命令的练习可从仓库根目录运行。

## 学习准备

从仓库根目录运行教程命令。仓库通过 [rust-toolchain.toml](../rust-toolchain.toml) 指定 Rust 工具链。浏览器 Demo 需要 Rust 和 C/C++ 编译器；示例服务器和 Python bindings 还需要 [OpenCV 等本地依赖](../docs/development.md#native-dependencies)。

如果暂时不装 OpenCV，可以先用 Demo 和 Rust 核心测试完成请求、模板、输出解析三个环节的练习。教程使用本地示例，不需要模型权重、GPU 或 API key。

命令采用 macOS / Linux 的 POSIX shell 写法。Windows 环境可用 WSL，或按开发指南调整命令。Demo 使用端口 7778，Python 和 Rust 示例服务器默认都使用 7777，同一时间启动其中一个。

## 先选一条动手路径

| 你目前的环境 | 从哪里开始 | 能观察到什么 |
| --- | --- | --- |
| 只准备 Rust 与 C/C++ 编译器 | 导览中的 Demo，第 01、02、06、07 章 | 请求转换、prompt 和手写模型输出的解析 |
| 有 Rust，想读数据经过每一层的变化 | 下方综合练习，第 01–05 章 | 实际 token IDs、预算结果、模拟输出和协议 chunk |
| 已安装 Python bindings 与 OpenCV | 第 04、05、08 章 | 自定义回调、HTTP 请求、SSE 与应用测试 |

Demo 的 Render 展示文本，Parse 处理粘贴的完整输出。token IDs 编解码请在综合练习或 tokenizer 示例里观察。GPU 调度、批处理、量化与模型权重计算属于推理引擎侧，可在掌握本项目的后端边界后继续学习。

## 可运行的 Rust 综合练习

在仓库根目录执行以下命令。它直接引用当前仓库的 Rust crate，并读取内置 tokenizer。这个独立练习有自己的 `[workspace]` 和 `Cargo.lock`。

```sh
CARGO_TARGET_DIR=target cargo run --manifest-path tutorials/examples/Cargo.toml --locked
```

默认只跑基础成功路径：实际 prompt、特殊字节区间、token IDs、预算 200、协议 chunk，最后正文是 `Hello world!`。输入 token 数来自词表；模拟输出的 token_count 是教学数据。

逐步增加条件时，在命令末尾添加场景名：

```sh
CARGO_TARGET_DIR=target cargo run --manifest-path tutorials/examples/Cargo.toml --locked -- stop
CARGO_TARGET_DIR=target cargo run --manifest-path tutorials/examples/Cargo.toml --locked -- thinking
CARGO_TARGET_DIR=target cargo run --manifest-path tutorials/examples/Cargo.toml --locked -- boundaries
```

| 场景 | 预期观察 | 对应章节 |
| --- | --- | --- |
| basic（默认） | 正文 Hello world!；top_p 转成 1.0；预算 200 | 01–04 |
| stop | 跨片段停止词使正文变为 Hell | 05 |
| thinking | 默认未闭合时只有 reasoning；关闭或闭合后出现正文 | 05、08 |
| boundaries | Messages 可省略上限；满窗口返回 1 等当前边界 | 03 |

[main.rs](examples/main.rs) 是完整教学实现；[tests.rs](examples/tests.rs)验证场景与解析边界。不要把测试中的“每段计一个 token”当成真实引擎用量。

```sh
CARGO_TARGET_DIR=target cargo test --manifest-path tutorials/examples/Cargo.toml --locked
python3 -m unittest discover -s tutorials/examples -p test_backend_adapter.py -v
```

第二条仅用 Python 标准库，验证第 10 章的离线适配器。第 08 章另有一个显式开启的失败复现命令，它预期失败；普通测试命令应全部通过。

## 常见术语从哪里查

| 术语 | 第一次重点学习 |
| --- | --- |
| API、反序列化、ConversationRequest | 01 请求转换 |
| Prompt、Chat template、特殊 token、prefix | 02 模板 |
| Tokenizer、context window、usage | 03 编码与预算 |
| InferenceChunk、Prefill、Decode | 04 后端；原理展开见 11 |
| State、delta、SSE、stop sequence | 05 输出解析 |
| DSML、tool call、JSON Schema、约束解码 | 06 工具 |
| ImageSource、占位符、quota | 07 图片 |
| trait、关联类型、move、borrow、Pin、Send | 09 Rust |
| 累计与增量、取消、资源清理 | 10 适配 |
| logits、softmax、KV cache、TTFT、TPOT | 11 原理 |

## 学习完成后，用任务验收

先独立完成，再参考章节答案：能画出请求各层数据；解释默认 thinking 与正文的关系；构造跨片段停止词测试；读懂请求到响应的类型关系；修复一个最小教学问题；实现累计文本适配并验证清理。

做到这些，说明你已能处理本项目一部分协议、模板和流式解析问题。若目标是维护真实推理服务，还要记录引擎版本、实际约束支持、取消和性能数据，按第 10、11 章完成环境相关验证。

## 找到负责某个行为的目录

| 目录 | 负责内容 | 主要章节 |
| --- | --- | --- |
| `deepseek-recipe-core` | Conversation、消息、工具、图片和 Prompt 片段类型 | 01、02、07 |
| `deepseek-recipe` | 三协议转换、输出解析、协议响应 | 01、05、06 |
| `deepseek-recipe-encoding` | V4/V4.1 模板与 token 编码 | 02、03 |
| `deepseek-recipe-image` | 图片读取、限制与预处理 | 07 |
| `deepseek-recipe-macros` | `prompt!` 等宏的实现 | 02，进阶时阅读 |
| `deepseek-recipe-python` | Rust 能力的 Python bindings 和类型声明 | 04、08 |
| `server-rs`、`server-py` | HTTP 与 mock 推理的组装 | 04、05 |
| `encoding-decoding-demo` | 浏览器里的渲染和解析实验 | 01、02、06、07 |

阅读源码时先从表中对应目录进入，再沿章节指定函数追踪。避免把某个默认行为推广为所有协议或所有模型的通用规则。

## 审校与验证范围

本轮更新日期：2026-11-17。教程结论分为当前源码行为、教学样例和真实后端接入指南。Rust 练习运行当前工作区的 crate；外部原理参考优先使用 Rust Book、Transformers 官方文档和 DistServe 原论文页面。

验证命令、实际结果和限制记在[维护记录](maintenance-plan.md)。其中真实 HTTP、OpenCV 图片预处理与 GPU 推理尚未端到端验证；离线适配器的清理测试也不能代替引擎资源观测。仓库版本更新后，应重新执行场景，核对默认值和协议差异。
