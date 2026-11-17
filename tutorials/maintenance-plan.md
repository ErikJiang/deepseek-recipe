# 教程优化实施记录

本页供文档维护者阅读，不属于初学者必读章节。

**目标：** 按 2026-11-17 的教程审阅建议，让学习者从观察请求流程进阶到解释源码、复现问题和实现适配。

**依据：** 当前仓库 `935817c`，以及本轮已确认的实施顺序。

**方法：** 保留 00–08 章主线；扩充关键章节，新增 Rust 导读、后端适配实验和推理原理附录。示例复用现有 Rust 依赖，Python 离线适配实验只用标准库。

## 实施清单

- [x] 纠正导览的 thinking 默认行为，补停止词和 JSON 解析的语义范围。
- [x] 将 Rust 练习分为基础、停止、思考和边界场景；增加分片与 token 来源的回归验证。
- [x] 为 01–03 章增加相同请求的数据快照、特殊区间和预算计算。
- [x] 为 05 章增加分层图、状态图、逐片段执行表与故障实验。
- [x] 新增 09 Rust 实践：所有权、关联类型、错误传播、异步流与资源释放。
- [x] 完善 08 章完整失败与修复案例、独立练习和验收标准。
- [x] 新增 10 后端适配：固定上游样本、增量与用量映射、结束和清理；真实引擎部分明确为未验证的接入指南。
- [x] 新增 11 推理原理：生成、采样、Prefill/Decode、KV cache、延迟与吞吐。
- [x] 统一阅读层次、术语入口、依赖说明、章节导航和验证记录。
- [x] 执行教学示例与测试、format、Clippy、文档链接和图表检查。

## 重点核验

| 容易误解的输入或边界 | 验证位置 |
| --- | --- |
| 默认 thinking + 未结束思考的 mock | Rust 思考场景与维护案例 |
| 停止词跨 chunk、匹配失败或出现在 reasoning | Rust 停止词与状态回归测试 |
| 相同标记来自 Text、普通 IDs 或特殊 ID | Rust token 来源回归测试 |
| 上游累计文本回退、漏发结束、消费者提前停止 | Python 离线适配实验 |
| 文档跑通 mock 却被理解成真实引擎已验证 | 每篇实验范围及课程验证表 |

## 实施边界

只修改教程和教学示例。真实 HTTP、OpenCV 图片预处理、模型权重和 GPU 性能需要相应环境，不能由 mock 结果代替。文中的技术结论区分当前源码行为、教学简化和待验证的接入方案。

## 验证记录

本轮执行日期：2026-11-17。以下结果来自当前工作区，离线 Cargo 检查使用了已存在的依赖缓存；读者首次构建仍可能需要下载依赖。

| 检查 | 结果 |
| --- | --- |
| Rust 教学测试 | 9 个通过，覆盖四场景、停止词范围、token 来源、UTF-8、工具往返、schema 边界 |
| Python 离线适配测试 | 9 个通过，包含提前关闭、等待期间取消与异常 |
| 核心库 / encoding / Demo 测试 | 82 个通过；出现 macOS SDK 环境警告，但没有测试失败 |
| Rust 示例 format | 通过 |
| Rust 示例 Clippy（all-targets，warnings 为错误） | 通过 |
| 教学故障重现 | 开启 TUTORIAL_REPRODUCE_BUG 时 content 契约按预期失败；关闭后通过 |
| Rust 编译错误实验 | 在临时副本中确认 E0382；项目示例保持可编译 |
| 文档结构与链接 | 14 篇 Markdown，177 个相对链接与锚点有效 |
| 示例核对 | 39 个 JSON/Python/shell 块语法通过；12 个 Rust 节选与当前源码匹配 |
| 原理计算 | 实际执行 softmax 示例，输出与表格一致 |
| 图表 | 10 个 Mermaid 图人工检查流程与结构；未进行图形渲染验证 |

主要复查命令：

```sh
CARGO_TARGET_DIR=target cargo test --manifest-path tutorials/examples/Cargo.toml --locked
CARGO_TARGET_DIR=target cargo clippy --manifest-path tutorials/examples/Cargo.toml --all-targets --locked -- -D warnings
cargo fmt --manifest-path tutorials/examples/Cargo.toml -- --check
python3 -m unittest discover -s tutorials/examples -p test_backend_adapter.py -v
cargo test -p deepseek-recipe-core -p deepseek-recipe -p deepseek-recipe-encoding -p encoding-decoding-demo --locked
```

链接和示例语法用本地 Pandoc AST 与 Python 检查，源码节选逐一与工作区文件对照。文档更新后应重新检查这些项；这组检查不证明所有自然语言描述都正确，因此还需要源码审阅。

独立审阅发现第 09 章原来的操作指引会先制造变量未定义错误，偏离所有权教学目的。已改成明确的替换片段，并在临时 crate 验证得到 E0382。该修改只涉及教学步骤，没有改动业务逻辑。

当前环境没有 cv2、deepseek_recipe Python 模块和 FastAPI。HTTP 桥接做了语法与 bindings 签名核对；真实 HTTP、多模态预处理、GPU 推理与性能未端到端验证。第 10 章真实引擎部分保持为接入指南，不声称存在已验证的引擎组合。

后续维护重点：版本变化时复核默认模式、usage、停止词范围；具备对应环境后补 HTTP 与图片实验记录；真实引擎接入必须记录权重、tokenizer、引擎版本及取消行为。
